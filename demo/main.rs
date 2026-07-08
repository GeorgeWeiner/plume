//! Demo file: Rust syntax highlighting showcase.

use std::collections::HashMap;

const MAX_RETRIES: u32 = 3;

#[derive(Debug, Clone)]
pub struct Feather {
    pub name: String,
    pub weight_grams: f64,
}

impl Feather {
    /// A feather so light it barely exists.
    pub fn new(name: &str) -> Self {
        Feather {
            name: name.to_string(),
            weight_grams: 0.0062,
        }
    }

    pub fn is_light(&self) -> bool {
        self.weight_grams < 1.0
    }
}

fn main() {
    let mut inventory: HashMap<String, Feather> = HashMap::new();

    for i in 0..MAX_RETRIES {
        let feather = Feather::new(&format!("plume-{i}"));
        println!("collected {:?} ({} g)", feather.name, feather.weight_grams);
        inventory.insert(feather.name.clone(), feather);
    }

    /* Block comments
       highlight across lines too. */
    let total: f64 = inventory.values().map(|f| f.weight_grams).sum();
    match total {
        t if t < 1.0 => println!("weightless: {t} g"),
        t => println!("heavy: {t} g"),
    }
}
