# Heap Allocation Demo for PY32F030

This is a demonstration of heap allocation on PY32F030 microcontrollers using the `[embedded-alloc](https://github.com/rust-embedded/embedded-alloc)` crate. The example shows how to implement dynamic memory allocation in a no_std environment.

## Memory Usage Analysis

`cargo nm --release -- --size-sort`

Installation:
```
$ cargo install cargo-binutils
$ rustup component add llvm-tools
```

### LLFF Implementation

- 1024 bytes configuration:
  ```
  0000001c b main::HEAP
  00000400 b main::____embassy_main_task::{{closure}}::HEAP_MEM
  ```

- 4096 bytes configuration:
  ```
  0000001c b main::HEAP
  00001000 b main::____embassy_main_task::{{closure}}::HEAP_MEM
  ```

### TLSF Implementation

- 1024 bytes configuration:
  ```
  00001088 b main::HEAP
  00000400 b main::____embassy_main_task::{{closure}}::HEAP_MEM
  ```

- 4096 bytes configuration:
  ```
  00001088 b main::HEAP
  00001000 b main::____embassy_main_task::{{closure}}::HEAP_MEM
  ```