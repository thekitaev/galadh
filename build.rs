//use tonic_build;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all("src/proto")?;
    tonic_build::configure()
        .out_dir("src/proto")
        .build_server(true)
        .build_client(true)
        .compile(&["proto/galadh.proto"], &["proto"])?;
    Ok(())
}
