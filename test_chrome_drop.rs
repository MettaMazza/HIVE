use headless_chrome::Browser;
fn main() {
    println!("Has close: {:?}", Browser::default().is_ok());
}
