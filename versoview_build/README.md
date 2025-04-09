# VeroView Build

This is a crate to help with getting started with using verso as a webview without building it yourself

## Example

To use it, first add it to your build dependency, and in your build script:

```rust
fn main() {
    versoview_build::download_and_extract_verso("output_directory").unwrap();
}
```
