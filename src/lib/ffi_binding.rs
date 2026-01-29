use magnus::{define_module, function, prelude::*, value::Opaque, Error, Ruby, Value};
use once_cell::sync::OnceCell;
use rb_sys::{rb_thread_call_with_gvl, rb_thread_call_without_gvl};
use std::time::Duration;
use std::{ffi::c_void, ptr::null_mut};
use tokio::runtime::Builder;
use tokio::sync::{mpsc, watch};
use tokio::task::LocalSet;

struct Handles {
    tx: mpsc::UnboundedSender<DispatchReq>,
    shutdown_tx: watch::Sender<bool>,
}

static HANDLES: OnceCell<Handles> = OnceCell::new();

pub fn run_without_gvl<F>(f: F) -> Result<(), Error>
where
    F: FnOnce() -> Result<(), Error> + Send + 'static,
{
    struct Data<F> {
        f: Option<F>,
        out: Option<Result<(), Error>>,
    }

    unsafe extern "C" fn work<F>(data: *mut c_void) -> *mut c_void
    where
        F: FnOnce() -> Result<(), Error> + Send + 'static,
    {
        let data = &mut *(data as *mut Data<F>);
        let f = data.f.take().unwrap();
        data.out = Some(f());
        null_mut()
    }

    // Optional: an “unblock” callback; you can keep it null for now.
    unsafe extern "C" fn unblock(_: *mut c_void) {}

    let mut data = Data {
        f: Some(f),
        out: None,
    };

    unsafe {
        rb_thread_call_without_gvl(
            Some(work::<F>),
            (&mut data as *mut Data<F>).cast::<c_void>(),
            Some(unblock),
            null_mut(),
        );
    }

    data.out.unwrap_or_else(|| Ok(()))
}

struct DispatchReq {
    store: Opaque<Value>,
    key: String,
    delay_secs: u64,
}

pub fn dispatcher_started() -> bool {
    HANDLES.get().is_some()
}

pub fn call_get(store: Value, key: String) -> Result<Option<String>, Error> {
    // optionally enforce the interface
    if !store.respond_to("get", false)? {
        return Err(Error::new(
            magnus::exception::type_error(),
            "store must respond to #get",
        ));
    }

    // call Ruby: store.get(key)
    let result: Option<String> = store.funcall("get", (key,))?;
    Ok(result)
}

pub fn call_get_delayed(store: Value, key: String) -> Result<(), Error> {
    let handles = HANDLES
        .get()
        .ok_or_else(|| Error::new(magnus::exception::runtime_error(), "dispatcher not started"))?;

    if !store.respond_to("get", false)? {
        return Err(Error::new(
            magnus::exception::type_error(),
            "store must respond to #get",
        ));
    }

    let req = DispatchReq {
        store: Opaque::from(store),
        key,
        delay_secs: 1,
    };

    handles
        .tx
        .send(req)
        .map_err(|_| Error::new(magnus::exception::runtime_error(), "dispatcher stopped"))?;
    Ok(())
}

pub fn start_dispatcher() -> Result<(), Error> {
    let (tx, mut rx) = mpsc::unbounded_channel::<DispatchReq>();
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    HANDLES.set(Handles { tx, shutdown_tx }).map_err(|_| {
        Error::new(
            magnus::exception::runtime_error(),
            "dispatcher already started",
        )
    })?;

    run_without_gvl(move || {
        let rt = Builder::new_current_thread().enable_time().build().unwrap();
        let local = LocalSet::new();

        rt.block_on(local.run_until(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            break;
                        }
                    }
                    maybe_req = rx.recv() => {
                        let Some(req) = maybe_req else { break; };
                        tokio::task::spawn_local(async move {
                            tokio::time::sleep(Duration::from_secs(req.delay_secs)).await;

                            unsafe extern "C" fn with_gvl(data: *mut c_void) -> *mut c_void {
                                let boxed: Box<DispatchReq> = Box::from_raw(data as *mut DispatchReq);

                                let ruby = Ruby::get().unwrap();
                                let store: Value = ruby.get_inner(boxed.store);

                                // You probably want to log errors instead of ignoring them:
                                let _ = store.funcall::<_, _, Value>("get", (boxed.key,));

                                null_mut()
                            }

                            let boxed = Box::new(req);
                            unsafe { rb_thread_call_with_gvl(Some(with_gvl), Box::into_raw(boxed) as *mut _); }
                        });
                    }
                }
            }
        }));

        Ok(())
    })
}

pub fn stop_dispatcher() -> Result<(), Error> {
    let handles = HANDLES
        .get()
        .ok_or_else(|| Error::new(magnus::exception::runtime_error(), "dispatcher not started"))?;

    handles
        .shutdown_tx
        .send(true)
        .map_err(|_| Error::new(magnus::exception::runtime_error(), "dispatcher stopped"))?;

    Ok(())
}

#[magnus::init]
fn init() -> Result<(), Error> {
    let module = define_module("StatsigFFI")?;
    module.define_module_function("start_dispatcher", function!(start_dispatcher, 0))?;
    module.define_module_function("stop_dispatcher", function!(stop_dispatcher, 0))?;
    module.define_module_function("dispatcher_started", function!(dispatcher_started, 0))?;
    module.define_module_function("call_get", function!(call_get, 2))?;
    module.define_module_function("call_get_delayed", function!(call_get_delayed, 2))?;
    Ok(())
}
