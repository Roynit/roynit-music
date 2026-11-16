//! Example trusted native plugin: shuffle only upcoming tracks, preserving the current slot.
use rand::seq::SliceRandom;
use std::io::{self, Read};
fn main() -> anyhow::Result<()> {
    let mut text = String::new();
    io::stdin()
        .take(8 * 1024 * 1024)
        .read_to_string(&mut text)?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    let queue = &value["params"]["queue"];
    let tracks = queue["tracks"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No queue"))?;
    let current = queue["index"].as_u64().map(|n| n as usize);
    let mut order: Vec<usize> = (0..tracks.len()).collect();
    let start = current.map_or(0, |i| (i + 1).min(order.len()));
    order[start..].shuffle(&mut rand::rng());
    println!(
        "{}",
        serde_json::json!({"message":"Shuffled upcoming tracks","commands":[{"command":"queue.reorder","params":{"order":order}}]})
    );
    Ok(())
}
