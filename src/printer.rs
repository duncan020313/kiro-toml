// TODO: full implementation in task 10
use crate::value::Value;

pub struct PrettyPrinter {
    output: String,
}

impl PrettyPrinter {
    pub fn new() -> Self {
        PrettyPrinter { output: String::new() }
    }

    pub fn print(value: &Value) -> String {
        let _ = value;
        // TODO
        String::new()
    }
}
