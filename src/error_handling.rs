use serde::{Deserialize, Serialize};
use std::sync::{OnceLock, Mutex};
use crate::index::Crate;
use std::fs::File;

#[derive(Serialize, Deserialize)]
pub(crate) struct ErrorData {
    pub(crate) name: String,
    origin: String
}

#[derive(Serialize, Deserialize)]
struct ErrorHandling {
    crates: Mutex<Vec<ErrorData>>,
}

static ERROR_HANDLE: OnceLock<ErrorHandling> = OnceLock::new();

fn get_handle() -> &'static ErrorHandling {
    ERROR_HANDLE.get_or_init(|| {
        ErrorHandling { crates: Mutex::new(vec![]) }
    })
}

pub fn handle_error(c : &Crate, origin: &str) {
    println!("Failed: [{}], Origin: {}", &c.name, origin);

    let handle = get_handle();
    let guard = handle.crates.lock();
    guard.unwrap().push(ErrorData {
        name: c.name.clone(),
        origin: origin.to_string()
    });
}

pub fn handle_error_raw(name: String, origin: &str) {
    println!("Failed: [{}], Origin: {}", &name, origin);

    let handle = get_handle();
    let guard = handle.crates.lock();
    guard.unwrap().push(ErrorData {
        name: name,
        origin: origin.to_string()
    });
}

pub fn handle_error_raw_name(name: String, origin: &str) {
    println!("Failed: [{}], Origin: {}", &name, origin);

    let handle = get_handle();
    let guard = handle.crates.lock();
    guard.unwrap().push(ErrorData {
        name: name,
        origin: origin.to_string()
    });
}

fn print_all() {
    let handle = get_handle();
    let guard = handle.crates.lock().unwrap();
    println!("Errored Crates:");
    guard.iter().for_each(|x| {
        println!("{}-{}", x.name, x.origin);
    });
}

pub fn flush() {
    let file = File::create("errors.json");
    if let Ok(f) = file {
        let handle = get_handle();
        if let Ok(_) = serde_json::to_writer(f, handle) {
            println!("Finished with {} crates that went unanalyzed.", handle.crates.lock().unwrap().len());
            return;
        }
    }
    print_all();
}
