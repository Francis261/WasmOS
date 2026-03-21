use anyhow::Result;
use wasmos_runtime::WasmOs;

#[tokio::main]
async fn main() -> Result<()> {
    let os = WasmOs::bootstrap().await?;
    os.shell.run_repl().await
}
