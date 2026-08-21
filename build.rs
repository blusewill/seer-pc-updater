// Source - https://stackoverflow.com/a/66839911
// Posted by frankenapps, modified by community. See post 'Timeline' for change history
// Retrieved 2026-08-21, License - CC BY-SA 4.0

extern crate winres;

fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("game.ico"); // Replace this with the filename of your .ico file.
        res.compile().unwrap();
    }
}
