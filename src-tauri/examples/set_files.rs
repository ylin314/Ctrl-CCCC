use clipboard_rs::{Clipboard, ClipboardContext};
fn main() {
    let clipboard = ClipboardContext::new().unwrap();
    clipboard
        .set_files(vec![
            "C:\\Users\\LinYang\\Documents\\Code\\Project\\Ctrl-CCCC\\Cargo.toml".to_string(),
            "C:\\Users\\LinYang\\Documents\\Code\\Project\\Ctrl-CCCC\\README.md".to_string(),
        ])
        .unwrap();
}
