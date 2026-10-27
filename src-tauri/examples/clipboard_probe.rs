use clipboard_rs::common::RustImage;
use clipboard_rs::{Clipboard, ClipboardContext, ContentFormat};

fn main() {
    let clipboard = ClipboardContext::new().unwrap();
    println!("formats = {:?}", clipboard.available_formats());
    println!("text = {}", clipboard.has(ContentFormat::Text));
    println!("image = {}", clipboard.has(ContentFormat::Image));
    println!("files = {}", clipboard.has(ContentFormat::Files));
    match clipboard.get_files() {
        Ok(files) => println!("file_list = {files:?}"),
        Err(error) => println!("file_error = {error}"),
    }
    match clipboard.get_image() {
        Ok(image) => println!("image_size = {:?}", image.get_size()),
        Err(error) => println!("image_error = {error}"),
    }
    match clipboard.get_text() {
        Ok(text) => println!("text_length = {}", text.chars().count()),
        Err(error) => println!("text_error = {error}"),
    }
}
