# Translate WASM modules into JVM class files

This is a library and CLI for converting any single WASM module into several JVM
class files (usually packaged into a JAR).

```
$ cargo run --bin wasm2jar foo.wasm --output-class 'me/Foo' --jar foo.jar
```

## Building and testing

The usual `cargo` commands apply for building and unit tests:

```bash
$ cargo build      # Compile everything
$ cargo test       # Run unit tests
```

Since the specification tests for WebAssembly is encoded in `.wast` files, there
is also a way to run against a bunch of `.wast` files at once. This assumes that
`javac` and `java` are installed.

```bash
$ cargo run --bin wast2jar -- tests/loop.wast   # Run the tests in `tests/loop.wast`
$ cargo run --bin wast2jar -- tests             # Run all `*.wast` tests in `tests`
```

## Debugging

Some handy tools/techniques for debugging

  * [`wat2wasm` and `wasm2wat`][0] (from WABT) for inspecting/manipulating WASM
  * `javap` (in JDK) for inspecting generated class files
  * `jshell` (in JDK 9+) for running the output (`jshelll --class-path foo.jar`)
  * [`cfr`][1] for decompiling JVM bytecode into Java code
  * `hexdump` or `xxd` for debugging serialized class files

## References

  * [WebAssembly Core Specification](https://webassembly.github.io/spec/core/)
  * [Specification repo (spec tests are here)](https://github.com/WebAssembly/spec)
  * [JVM specification](https://docs.oracle.com/javase/specs/jvms/se17/html/index.html)

[0]: https://github.com/WebAssembly/wabt
[1]: https://www.benf.org/other/cfr/
