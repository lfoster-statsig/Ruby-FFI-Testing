use magnus::{define_module, function, prelude::*, Error, Value};

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

#[magnus::init]
fn init() -> Result<(), Error> {
    let module = define_module("StatsigFFI")?;
    module.define_module_function("call_get", function!(call_get, 2))?;
    Ok(())
}
