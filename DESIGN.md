# Types

## Basic types

Basic types get mapped onto their JVM equivalents. Value types correspond to
existing JVM primitive types.

| WASM type     | JVM type                        | Boxed JVM type      |
| ------------- | ------------------------------- | ------------------- |
| `i32`         | `int`                           | `java.lang.Integer` |
| `i64`         | `long`                          | `java.lang.Long`    |
| `f32`         | `float`                         | `java.lang.Float`   |
| `f64`         | `double`                        | `java.lang.Double`  |
| `funcref`     | `java.lang.invoke.MethodHandle` | already boxed       |
| `externref`   | `java.lang.Object`              | already boxed       |

This mapping is convenient since `retype` (`funcref` and `externref`) are
both JVM references, so they can be null.

## Function and block types

WASM blocks and functions can accept/return any number of inputs/outputs. OTOH,
the JVM imposes stricter limits:

 * [methods can take maximum 255 arguments][0] with `long` and `double`
   counting as two arguments (and the receiver also counting)

 * methods can return at most one value

We get around this constraint by packing extra arguments or returns into
arrays of objects (containing the boxed JVM types corresponding the WASM types).
For example, a function returning `(i32 i64 f32)` will return an `Object[]`
which will always three elements of type `Integer`, `Long`, and `Float`.

# Tables

Tables are represented using JVM arrays. Tables of functions are arrays of
`MethodHandle`s while tables of external references are arrays of `Object`.

Problem: JVM arrays are indexed using `int`, so are at most (2^32 - 1) elements
         OTOH, WASM tables can be up to 2^32 elements long.

# Memory

Memory is represented using `java.nio.ByteBuffer`. This is because:

  - `ByteBuffer` allows unaligned access
  - `ByteBuffer` has low-overhead access (see direct bytebuffers)
  - `ByteBuffer` has good `VarHandle` for more access modes (eg. atomic)

Problem: `ByteBuffer`s are at most (2^32 - 1) elements , so are at most (2^32 - 1) elements
         OTOH, WASM tables can be up to 2^32 elements long.

# Imports/Exports

Idea:

  - all imports/exports are just `java.lang.Object` arguments, so the entire
    set of imports can have type `Map<String, Object>`

  - since tables being resized, memories being resized, globals being set all
    entail interior mutability, the actual array, bytebuffer, etc. need to just
    be fields on some other object that represents the import/export

  - in order to avoid needing some shared library of types across WASM modules
    (eg. for when one module's imports are another's exports), we want to stick
    to vanilla Java types. One way to make this work is to reflectively lookup
    the `MethodHandle`s needed to get/set etc. the imports. This means that we
    can establish conventions like:

      * globals are objects with a `global` field of the right type
      * tables are objects with a `table` field of the right type
      * memories are objects with a `memory` field of the right type
      * functions are `MethodHandle`s of the right type

  - exporting globals means creating classes to carry the globals, tables,
    and memories. Wwe can share these when multiple exports are structurally
    the same (eg. two globals of type `i32`)

Internally, store just method handles for getting/setting the fields

[0]: https://docs.oracle.com/javase/specs/jvms/se16/html/jvms-4.html#jvms-4.11
