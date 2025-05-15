#![deny(missing_debug_implementations)]

pub mod client;
pub use workbook_icd as icd;

pub async fn read_line() -> String {
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        line
    })
    .await
    .unwrap()
}
