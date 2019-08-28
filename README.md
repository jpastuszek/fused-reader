[![Latest Version]][crates.io] [![Documentation]][docs.rs] ![License]

When working with data pipes it is often necessary to distinguish between EOF on the reader side caused by writer thread finishing write and writer panicking. This crate provides fused reader type that if writer thread dies while holding armed fuse the reader will get `BrokenPipe` error.

Fuses can also be blown with custom error that is passed to the reader end.

Example usage
=============

Writer panics and reader gets `BrokenPipe` error.

```rust
use pipe::pipe;
use fused_reader::fuse;
use std::io::{Read, Write, ErrorKind};
use std::thread;

let (reader, mut writer) = pipe();
let (mut reader, fuse) = fuse(reader);

thread::spawn(move || {
	let _fuse = fuse.arm().unwrap();
	writer.write(&[1]).unwrap();
	panic!("boom");
});

let mut data = Vec::new();

assert_eq!(reader.read_to_end(&mut data).unwrap_err().kind(), ErrorKind::BrokenPipe);
assert_eq!(&data, &[1]); // data that was written before panic
```

Writer fails with error passed to reader.

```rust
use pipe::pipe;
use fused_reader::fuse;
use std::io::{Read, Write, Error as IoError, ErrorKind};
use std::thread;

let (reader, mut writer) = pipe();
let (mut reader, fuse) = fuse(reader);

thread::spawn(move || {
	let fuse = fuse.arm().unwrap();
	writer.write(&[1]).unwrap();
	fuse.blow(IoError::new(ErrorKind::UnexpectedEof, "uh! oh!"))
});

let mut data = Vec::new();

assert_eq!(reader.read_to_end(&mut data).unwrap_err().kind(), ErrorKind::UnexpectedEof);
assert_eq!(&data, &[1]); // data that was written before error
```

[crates.io]: https://crates.io/crates/fused-reader
[Latest Version]: https://img.shields.io/crates/v/fused-reader.svg
[Documentation]: https://docs.rs/fused-reader/badge.svg
[docs.rs]: https://docs.rs/fused-reader
[License]: https://img.shields.io/crates/l/fused-reader.svg
