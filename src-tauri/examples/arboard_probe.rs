fn main() {
    let mut clipboard = arboard::Clipboard::new().unwrap();
    match clipboard.get_image() {
        Ok(image) => println!("arboard image = {}x{}", image.width, image.height),
        Err(error) => println!("arboard error = {error}"),
    }
}
