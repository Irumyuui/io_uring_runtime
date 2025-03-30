# io_uring_rt

Try to wrapping `io_uring`.

## Example

```rust
use io_uring_rt::Ring;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let file = std::fs::File::open("./Cargo.toml").unwrap();
    let file_size = file.metadata().unwrap().len();

    let ring = Ring::new(1024).unwrap();
    let mut buf = vec![0_u8; file_size as usize];

    let result = ring.read_at(&file, &mut buf, 0).await.unwrap();
    println!("result: {}", result);

    println!("read: ");
    println!("{}", String::from_utf8(buf)?);

    Ok(())
}
```
