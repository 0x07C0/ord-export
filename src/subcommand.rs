use std::io::BufWriter;
use sha3::{Sha3_256, Digest};
use rustc_serialize::hex::ToHex;
use indicatif::{ProgressBar, ProgressStyle};

use super::*;

pub mod epochs;
pub mod find;
mod index;
pub mod info;
pub mod list;
pub mod parse;
mod preview;
mod server;
pub mod subsidy;
pub mod supply;
pub mod traits;
pub mod wallet;

fn print_json(output: impl Serialize) -> Result {
  serde_json::to_writer_pretty(io::stdout(), &output)?;
  println!();
  Ok(())
}

#[derive(Debug, Parser)]
pub(crate) enum Subcommand {
  #[clap(about = "List the first satoshis of each reward epoch")]
  Epochs,
  #[clap(about = "Run an explorer server populated with inscriptions")]
  Preview(preview::Preview),
  #[clap(about = "Find a satoshi's current location")]
  Find(find::Find),
  #[clap(about = "Update the index")]
  Index,
  #[clap(about = "Display index statistics")]
  Info(info::Info),
  #[clap(about = "List the satoshis in an output")]
  List(list::List),
  #[clap(about = "Parse a satoshi from ordinal notation")]
  Parse(parse::Parse),
  #[clap(about = "Display information about a block's subsidy")]
  Subsidy(subsidy::Subsidy),
  #[clap(about = "Run the explorer server")]
  Server(server::Server),
  #[clap(about = "Display Bitcoin supply information")]
  Supply,
  #[clap(about = "Display satoshi traits")]
  Traits(traits::Traits),
  #[clap(subcommand, about = "Wallet commands")]
  Wallet(wallet::Wallet),
  #[clap(about = "Export text records to a csv file")]
  Export,
}

impl Subcommand {
  pub(crate) fn run(self, options: Options) -> Result {
    match self {
      Self::Epochs => epochs::run(),
      Self::Preview(preview) => preview.run(),
      Self::Find(find) => find.run(options),
      Self::Index => index::run(options),
      Self::Info(info) => info.run(options),
      Self::List(list) => list.run(options),
      Self::Parse(parse) => parse.run(),
      Self::Subsidy(subsidy) => subsidy.run(),
      Self::Server(server) => {
        let index = Arc::new(Index::open(&options)?);
        let handle = axum_server::Handle::new();
        LISTENERS.lock().unwrap().push(handle.clone());
        server.run(options, index, handle)
      }
      Self::Supply => supply::run(),
      Self::Traits(traits) => traits.run(),
      Self::Wallet(wallet) => wallet.run(options),
      Self::Export => {
        let index = Index::open(&options)?;
        let file_name = chrono::offset::Utc::now().format("%d-%m-%Y_%H-%M.csv");
        let file_name = format!("{file_name}");
        let file = std::fs::File::create(file_name)?;
        let buffer = BufWriter::new(file);
        let mut csv = csv::Writer::from_writer(buffer);
        csv.write_record(&["hash", "timestamp", "text", "link"])?;
        let ref mut from = None;
        let (_, prev, _) = index.get_latest_inscriptions_with_prev_and_next(1, None)?;
        let progress_bar = ProgressBar::new(prev.expect("No inscriptions found."));
        progress_bar.set_position(0);
        progress_bar.set_style(
          ProgressStyle::with_template("[exporting] {wide_bar} {pos}/{len}").unwrap(),
        );
        let mut seen = std::collections::hash_set::HashSet::new();
        loop {
          let (inscs, prev, _) = index.get_latest_inscriptions_with_prev_and_next(1000, *from)?;
          if prev == None {
            break;
          }
          *from = prev;
          for insc in inscs {
            let insc_data = index.get_inscription_by_id(insc)?;
            let insc_time = index.get_inscription_entry(insc)?;
            if let Some(data) = insc_data {
              match data.media() {
                Media::Text => {
                  if let Some(text) = data.body() {
                    let mut hasher = Sha3_256::new();
                    hasher.update(text);
                    let hash = hasher.finalize();
                    let text = String::from_utf8_lossy(text).to_string();
                    if seen.contains(&text) {
                      continue;
                    }
                    seen.insert(text.clone());
                    let datetime = chrono::NaiveDateTime::from_timestamp_millis(
                      insc_time.unwrap().timestamp as i64 * 1000,
                    )
                    .unwrap();
                    let datetime = DateTime::<Utc>::from_utc(datetime, Utc);
                    csv.write_record(&[
                      hash[..].to_hex(),
                      datetime.to_rfc3339(),
                      text,
                      format!("https://ordinals.com/inscription/{insc}"),
                    ])?;
                  }
                }
                _ => { /* Ignore non-text records */ }
              }
            }
            progress_bar.inc(1);
          }
          csv.flush()?;
        }
        progress_bar.finish_and_clear();
        Ok(())
      }
    }
  }
}
