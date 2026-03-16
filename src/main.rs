#![allow(unexpected_cfgs)]

use hive::run;

#[cfg(not(tarpaulin_include))]
#[tokio::main]
async fn main() {
    run().await;
}
