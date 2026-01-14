# frozen_string_literal: true

require "rbconfig"

def require_native_extension
  base = File.expand_path("..", __dir__)
  lib_base = "ffi_ruby_testing"
  # Ruby often uses ".bundle" on macOS and ".so" on Linux, but the Rust cdylib
  # can emit ".dylib" on macOS, so include a fallback list.
  ruby_exts = [RbConfig::CONFIG["DLEXT"], RbConfig::CONFIG["DLEXT2"]].compact.uniq
  exts = (ruby_exts + %w[dylib so]).compact.uniq
  preferred_ext = ruby_exts.first
  builds = %w[release debug]
  basenames = [lib_base, "lib#{lib_base}"]

  last_error = nil
  builds.each do |build|
    exts.each do |ext|
      basenames.each do |basename|
        candidate = File.join(base, "target", build, "#{basename}.#{ext}")
        next unless File.exist?(candidate)

        # Prefer to load using the unprefixed base name and the platform Ruby
        # extension (so the Init_ffi_ruby_testing symbol is found).
        load_ext = preferred_ext || ext
        load_target = File.join(base, "target", build, "#{lib_base}.#{load_ext}")

        if load_target != candidate && !File.exist?(load_target)
          begin
            File.symlink(candidate, load_target)
          rescue StandardError
            # If symlink fails, fall back to loading the original filename.
            load_target = candidate
          end
        elsif load_target != candidate
          # A different file already exists at the preferred name; use it.
          load_target = load_target
        end

        begin
          require load_target
          return
        rescue LoadError => e
          last_error = e
        end
      end
    end
  end

  raise(last_error || LoadError.new("native extension not built; run `cargo build --release` first"))
end

require_native_extension

module Statsig
  class DataStore
    def get(key)
      raise NotImplementedError
    end

    def set(key, value)
      raise NotImplementedError
    end
  end
end

class MyStore < Statsig::DataStore
  def get(key)
    print(key + "\n")
    print("\n")
    "value_for_#{key}"
  end

  def set(key, value)
    # placeholder no-op
    value
  end
end

if $PROGRAM_NAME == __FILE__
  store = MyStore.new
  result = StatsigFFI.call_get(store, "example-key")
  warn "StatsigFFI.call_get returned: #{result.inspect}"
end
