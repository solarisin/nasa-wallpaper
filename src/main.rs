/*
   Copyright 2019-2026 David Población Criado

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       https://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
*/

extern crate chrono;
extern crate chrono_tz;
extern crate clap;
extern crate colored;
extern crate json;
extern crate rand;
extern crate reqwest;
extern crate wallpaper;

#[macro_use]
extern crate serde_derive;

use core::option::Option;
use chrono::prelude::*;
use chrono_tz::Tz;
use chrono_tz::US::Eastern;
use clap::{builder::PossibleValue, Arg, Command, ValueEnum};
use colored::*;
use rand::Rng;
use std::borrow::Cow;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::process;

const LICENSE_TEXT: &str = r#"
   Copyright 2019-2026 David Población Criado

   Licensed under the Apache License, Version 2.0 (the 'License');
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       https://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
"#;

const VERSION: &str = "2.1.1";
const MSG_DONE: &str = "Done";
const MSG_CHANGING: &str = "Changing wallpaper...";
const URL_UNSPLASH: &str = "https://source.unsplash.com/user/nasa";

type WallpaperResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Debug)]
pub enum Mode {
    Center,
    Crop,
    Fit,
    Span,
    Stretch,
    Tile,
}

impl Mode {
    pub fn possible_values() -> impl Iterator<Item = PossibleValue> {
        Self::value_variants()
            .iter()
            .filter_map(ValueEnum::to_possible_value)
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.to_possible_value()
            .expect("no values are skipped")
            .get_name()
            .fmt(f)
    }
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        for variant in Self::value_variants() {
            if variant.to_possible_value().unwrap().matches(s, false) {
                return Ok(variant.clone());
            }
        }
        Err(format!("invalid variant: {s}"))
    }
}

impl ValueEnum for Mode {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Center,
            Self::Crop,
            Self::Fit,
            Self::Span,
            Self::Stretch,
            Self::Tile,]
    }
    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            Self::Center => PossibleValue::new("center").alias("Center"),
            Self::Crop => PossibleValue::new("crop").alias("Crop"),
            Self::Fit => PossibleValue::new("fit").alias("Fit"),
            Self::Span => PossibleValue::new("span").alias("Span"),
            Self::Stretch => PossibleValue::new("stretch").alias("Stretch"),
            Self::Tile => PossibleValue::new("tile").alias("Tile"),
        })
    }
}

fn conv_mode(mode: &Mode) -> wallpaper::Mode {
    match mode {
        Mode::Center => {wallpaper::Mode::Center},
        Mode::Crop => {wallpaper::Mode::Crop},
        Mode::Fit => {wallpaper::Mode::Fit},
        Mode::Span => {wallpaper::Mode::Span},
        Mode::Stretch => {wallpaper::Mode::Stretch},
        Mode::Tile => {wallpaper::Mode::Tile},        
    }
}

#[derive(Deserialize)]
struct Apod {
    #[serde(default)]
    copyright: Option<String>,
    #[serde(default)]
    credit: Option<String>,
    date: String,
    explanation: String,
    #[serde(default, rename = "hdurl")]
    image_url: Option<String>,
    media_type: String,
    title: String,
    #[serde(rename = "url")]
    page_url: String,
}

impl fmt::Display for Apod {
    /// Formats an [`Apod`] instance for terminal output.
    ///
    /// This is the text shown by `println!("{apod}")`.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Title: {}\nDate: {}\nExplanation: {}\nCopyright: {}",
            self.title.bold().italic(),
            self.date.italic(),
            self.explanation,
            self.copyright.as_deref().unwrap_or("")
        )
    }
}

#[derive(Deserialize)]
struct NasaImage {
    nasa_id: String,
    title: String,
    center: String,
    description: String,
    date: String,
    url: String,
}

impl fmt::Display for NasaImage {
    /// Formats a [`NasaImage`] instance for terminal output.
    ///
    /// This is the text shown by `println!("{nasa_image}")`.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Title: {}\nDate: {}\nExplanation: {}\nCenter: {}\nNASA id: {}",
            self.title.bold().italic(),
            self.date.italic(),
            self.description,
            self.center,
            self.nasa_id
        )
    }
}

/// Fetches a single APOD Basic entry for a validated calendar date.
fn get_apod(date: &str) -> WallpaperResult<Apod> {
    get_apod_from(date, "https://science.nasa.gov/wp-json/wp/v2/apod-basic")
}

fn parse_apod_date(date: &str) -> WallpaperResult<NaiveDate> {
    if date.len() != 10 || !date.bytes().enumerate().all(|(index, byte)| {
        if index == 4 || index == 7 { byte == b'-' } else { byte.is_ascii_digit() }
    }) {
        return Err(format!("Invalid APOD date '{date}': expected YYYY-MM-DD").into());
    }
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| format!("Invalid APOD date '{date}': expected a calendar date YYYY-MM-DD").into())
}

/// The private request boundary also allows deterministic local HTTP tests.
fn get_apod_from(date: &str, base_url: &str) -> WallpaperResult<Apod> {
    let requested = parse_apod_date(date)?;
    let request_url = format!("{}/{}", base_url.trim_end_matches('/'), requested.format("%y%m%d"));
    let response = reqwest::blocking::get(&request_url)
        .map_err(|err| format!("APOD request failed: {err}"))?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(format!("No APOD has been published for {date} yet.").into());
    }
    if !status.is_success() {
        let body = response.text()
            .map_err(|err| format!("APOD HTTP {status}: could not read response: {err}"))?;
        let detail = match json::parse(&body) {
            Ok(error) if error["message"].as_str().is_some() => {
                let message = render_apod_text(error["message"].as_str().unwrap())?;
                match error["code"].as_str() {
                    Some(code) => format!("{code}: {message}"),
                    None => message,
                }
            }
            _ => body.trim().to_owned(),
        };
        return Err(format!("APOD HTTP {status} for {date}: {detail}").into());
    }
    let mut apod = response.json::<Apod>()
        .map_err(|err| format!("Invalid APOD response for {date}: {err}"))?;
    if apod.date != date {
        return Err(format!("APOD date mismatch: requested {date}, returned {}", apod.date).into());
    }
    apod.title = render_apod_text(&apod.title)?;
    apod.explanation = remove_metadata_label(render_apod_text(&apod.explanation)?, &["Explanation:"]);
    let copyright = remove_metadata_label(
        render_apod_text(apod.copyright.as_deref().unwrap_or(""))?,
        &["Image Credit & Copyright:", "Image Credit:", "Credit:", "Copyright:"],
    );
    let attribution = if copyright.is_empty() {
        render_apod_text(apod.credit.as_deref().unwrap_or(""))?
    } else {
        copyright
    };
    apod.copyright = Some(remove_metadata_label(attribution, &["Image Credit & Copyright:", "Image Credit:", "Credit:", "Copyright:"]));
    Ok(apod)
}

fn render_apod_text(html: &str) -> WallpaperResult<String> {
    html2text::config::with_decorator(html2text::render::TrivialDecorator::new())
        .string_from_read(html.as_bytes(), 100)
        .map(|mut text| {
            text.truncate(text.trim_end().len());
            let leading = text.len() - text.trim_start().len();
            text.drain(..leading);
            text
        })
        .map_err(|err| format!("Could not render APOD metadata: {err}").into())
}

fn remove_metadata_label(mut text: String, labels: &[&str]) -> String {
    for label in labels {
        if let Some(rest) = text.strip_prefix(label) {
            let leading = text.len() - rest.trim_start().len();
            text.drain(..leading);
            return text;
        }
    }
    text
}

/// Selects only eligible images; default downloads borrow the exact API URL.
fn apod_image_url(apod: &Apod, low: bool) -> WallpaperResult<Option<Cow<'_, str>>> {
    if apod.media_type != "image" {
        return Ok(None);
    }
    let image_url = apod.image_url.as_deref().filter(|url| !url.trim().is_empty())
        .ok_or_else(|| format!("APOD image unavailable for {}. Article: {}", apod.date, apod.page_url))?;
    if !low {
        return Ok(Some(Cow::Borrowed(image_url)));
    }
    let unavailable = || format!("Low-resolution rendition unavailable for {}. Article: {}", apod.date, apod.page_url);
    let mut url = reqwest::Url::parse(image_url).map_err(|_| unavailable())?;
    if url.scheme() != "https" || url.host_str() != Some("assets.science.nasa.gov")
        || !url.username().is_empty() || url.password().is_some() || url.port().is_some()
    {
        return Err(unavailable().into());
    }
    let path = url.path();
    if let Some(asset) = path.strip_prefix("/content/dam/").filter(|asset| !asset.is_empty()) {
        let path = format!("/dynamicimage/assets/{asset}");
        url.set_path(&path);
    } else if !path.strip_prefix("/dynamicimage/assets/").is_some_and(|asset| !asset.is_empty()) {
        return Err(unavailable().into());
    }
    let mut width = false;
    let mut height = false;
    let mut pairs = Vec::new();
    for (name, value) in url.query_pairs() {
        let value = match name.as_ref() {
            "w" | "h" => {
                let dimension = if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
                    value.parse::<u32>().ok().filter(|dimension| *dimension > 0)
                } else {
                    None
                }.ok_or_else(unavailable)?;
                let seen = if name == "w" { &mut width } else { &mut height };
                if *seen { return Err(unavailable().into()); }
                *seen = true;
                dimension.min(1280).to_string()
            }
            "fit" => continue,
            _ => value.into_owned(),
        };
        pairs.push((name.into_owned(), value));
    }
    if !width || !height {
        return Err(unavailable().into());
    }
    url.query_pairs_mut().clear().extend_pairs(pairs).append_pair("fit", "clip");
    Ok(Some(Cow::Owned(url.into())))
}

/// Fetches a random image from the NASA Image and Video Library.
///
/// This performs a search against `https://images-api.nasa.gov/search` and then
/// chooses a random result (potentially from a random page).
///
/// # Arguments
/// - `q`: Free text search terms.
/// - `center`: NASA center which published the media.
/// - `location`: Terms to search for in “Location” fields.
/// - `nasa_id`: The media asset’s NASA ID.
/// - `photographer`: The primary photographer’s name.
/// - `title`: Terms to search for in “Title” fields.
/// - `year_start`: Start year for results (`YYYY`).
/// - `year_end`: End year for results (`YYYY`).
///
/// # Returns
/// A [`NasaImage`] struct containing the selected item's metadata and a direct URL.
///
/// # Panics
/// This function uses `expect`/`unwrap` internally and may panic on network,
/// parsing, or response-shape errors.
///
/// # Exits
/// If the search yields zero results, this prints a message and terminates the
/// process with a non-zero exit code.
#[allow(clippy::too_many_arguments)]
fn get_nasa_image(
    q: &str,
    center: &str,
    location: &str,
    nasa_id: &str,
    photographer: &str,
    title: &str,
    year_start: &str,
    year_end: &str,
) -> NasaImage {
    let mut request_url = format!(
        "https://images-api.nasa.gov/search?media_type=image&q={q}&center={center}&location={location}&nasa_id={nasa_id}&photographer={photographer}&title={title}&year_start={year_start}&year_end={year_end}",
        q = q,
        center = center,
        location = location,
        nasa_id = nasa_id,
        photographer = photographer,
        title = title,
        year_start = year_start,
        year_end = year_end
    );

    let response_text = reqwest::blocking::get(&request_url)
        .expect("Failed to fetch NASA image")
        .text()
        .unwrap();
    let mut response_json = json::parse(&response_text).unwrap();

    let num_hits = response_json["collection"]["metadata"]["total_hits"]
        .as_usize()
        .unwrap_or(0);

    print!("Number of results: {}, {}", num_hits, request_url);
    if num_hits == 0 {
        println!("Couldn't find the file you're looking for. Try another tag.");
        process::exit(0x0100);
    }

    let pages = if (num_hits / 100) - 1 <= 100 {
        (num_hits / 100) - 1
    } else {
        100
    };
    let mut rng = rand::rng();
    let index_page = rng.random_range(0..=pages);
    request_url.push_str(&format!("&page={}", index_page));

    let response_text = reqwest::blocking::get(&request_url)
        .expect("Failed to fetch NASA image page")
        .text()
        .unwrap();
    response_json = json::parse(&response_text).unwrap();

    let index = if num_hits < 7 {
        num_hits - 1
    } else {
        rng.random_range(0..100)
    };
    let items = &response_json["collection"]["items"];
    let item = &items[index];
    let data = &item["data"][0];
    let url_collection = item["href"].as_str().unwrap();

    let response_collection = json::parse(
        &reqwest::blocking::get(url_collection)
            .unwrap()
            .text()
            .unwrap(),
    )
    .unwrap();

    let mut date = data["date_created"].as_str().unwrap().to_owned();
    date.truncate(10);

    NasaImage {
        nasa_id: data["nasa_id"].as_str().unwrap().to_owned(),
        title: data["title"].as_str().unwrap().to_owned(),
        center: data["center"].as_str().unwrap().to_owned(),
        description: data["description"].as_str().unwrap().to_owned(),
        date,
        url: response_collection[0].as_str().unwrap().to_owned(),
    }
}

/// Downloads the selected image and applies the existing wallpaper mode.
fn set_wallpaper(image_url: &str, mode: &Mode) -> WallpaperResult<()> {
    wallpaper::set_from_url(image_url)?;
    wallpaper::set_mode(conv_mode(mode))?;
    Ok(())
}

fn show_apod(apod: &Apod, info: bool, low: bool, mode: &Mode) -> WallpaperResult<()> {
    println!("{apod}");
    if apod.media_type != "image" {
        println!("{}", format!("This APOD is not an image. Article: {}", apod.page_url).yellow());
        return Ok(());
    }
    if info {
        return Ok(());
    }
    let image_url = apod_image_url(apod, low)?.expect("image media was checked");
    println!("{}", MSG_CHANGING.yellow());
    set_wallpaper(&image_url, mode)?;
    println!("{}", MSG_DONE.green());
    Ok(())
}

/// Prints the program license text to stdout.
fn print_license() {
    println!("{}", LICENSE_TEXT);
}

/// Returns today's date in US Eastern time (EST/EDT).
///
/// The APOD "day" is keyed off US Eastern time, so using this avoids fetching
/// "tomorrow" in other time zones.
fn get_today_est() -> (i32, u32, u32) {
    let est_now: DateTime<Tz> = Utc::now().with_timezone(&Eastern);
    (est_now.year(), est_now.month(), est_now.day())
}

/// Builds the CLI definition (commands/flags) for `nasa-wallpaper`.
fn cli() -> Command {
    Command::new("nasa-wallpaper")
        .version(VERSION)
        .author("David Población Criado")
        .about("Change desktop wallpaper with NASA images")
        .arg_required_else_help(true)
        .subcommand(
            Command::new("apod")
                .about("Get the APOD (Astronomical Picture of the Day)")
                .arg(Arg::new("date").short('d').long("date").value_name("DATE"))
                .arg(
                    Arg::new("low")
                        .short('l')
                        .long("low")
                        .help("Use the low definition image. It is faster than the HD photo")
                        .action(clap::ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("info")
                        .short('i')
                        .long("info")
                        .help("Show the APOD information in terminal instead of setting it as wallpaper")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("nasa_image")
                .about("Get a random image from the NASA Image Library (https://images.nasa.gov)")
                .arg(
                    Arg::new("query")
                        .short('q')
                        .long("query")
                        .value_name("Q")
                        .action(clap::ArgAction::Set)
                        .help("Free text search terms to compare to all indexed metadata"),
                )
                .arg(
                    Arg::new("center")
                        .short('c')
                        .long("center")
                        .value_name("CENTER")
                        .action(clap::ArgAction::Set)
                        .help("NASA center which published the media"),
                )
                .arg(
                    Arg::new("location")
                        .short('o')
                        .long("location")
                        .value_name("LOCATION")
                        .action(clap::ArgAction::Set)
                        .help("Terms to search for in “Location” fields"),
                )
                .arg(
                    Arg::new("nasa_id")
                        .short('i')
                        .long("nasa_id")
                        .value_name("NASA_ID")
                        .action(clap::ArgAction::Set)
                        .help("The media asset’s NASA ID"),
                )
                .arg(
                    Arg::new("photographer")
                        .short('p')
                        .long("phtographer")
                        .value_name("PHOTOGRAPHER")
                        .action(clap::ArgAction::Set)
                        .help("The primary photographer’s name"),
                )
                .arg(
                    Arg::new("title")
                        .short('t')
                        .long("title")
                        .value_name("TITLE")
                        .action(clap::ArgAction::Set)
                        .help("Terms to search for in “Title” fields"),
                )
                .arg(
                    Arg::new("year_start")
                        .long("year_start")
                        .value_name("YEAR_START")
                        .action(clap::ArgAction::Set)
                        .help("The start year for results. Format: YYYY"),
                )
                .arg(
                    Arg::new("year_end")
                        .long("year_end")
                        .value_name("YEAR_END")
                        .action(clap::ArgAction::Set)
                        .help("The end year for results. Format: YYYY"),
                ),
        )
        .subcommand(Command::new("unsplash").about(
            "Get a random image from the NASA's account in Unsplash (https://unsplash.com/@nasa)",
        ))
        .subcommand(Command::new("license").about("Print the license of this program"))
        .arg(
            Arg::new("mode")
                .short('m')
                .long("mode")
                .value_parser(clap::builder::EnumValueParser::<Mode>::new())
                .help("Sets the wallpaper display mode."),
        )
}

/// Normalizes arguments for backwards-compatible shorthand flags.
///
/// Converts legacy forms like `nasa-wallpaper -a ...` into
/// `nasa-wallpaper apod ...` and similarly `-n` to `nasa_image`.
fn normalize_args(mut args: Vec<OsString>) -> Vec<OsString> {
    // Backwards-compatible shorthand flags:
    // `nasa-wallpaper -a ...` => `nasa-wallpaper apod ...`
    // `nasa-wallpaper -n ...` => `nasa-wallpaper nasa_image ...`
    if args.len() >= 2 {
        if args[1] == OsStr::new("-a") {
            args[1] = OsString::from("apod");
        } else if args[1] == OsStr::new("-n") {
            args[1] = OsString::from("nasa_image");
        }
    }
    args
}

/// Program entry point.
///
/// Parses CLI arguments and dispatches to the chosen subcommand.
fn main() {
    let args = normalize_args(std::env::args_os().collect());
    let matches = cli().get_matches_from(args);
    let mode = matches.get_one::<Mode>("mode").unwrap_or(&Mode::Crop);

    match matches.subcommand() {
        Some(("apod", sub_matches)) => {
            let (year, month, day) = get_today_est();
            let today = format!("{year:04}-{month:02}-{day:02}");
            let date = sub_matches
                .get_one::<String>("date")
                .map(|s| s.as_str())
                .unwrap_or(&today);
            let low = sub_matches.get_flag("low");
            let info = sub_matches.get_flag("info");

            let result = get_apod(date)
                .and_then(|apod| show_apod(&apod, info, low, mode));
            if let Err(err) = result {
                eprintln!("{}", format!("Warning: {err}").yellow());
            }
        }
        Some(("unsplash", _)) => {
            println!("{}", MSG_CHANGING.yellow());
            wallpaper::set_from_url(URL_UNSPLASH).unwrap();
            wallpaper::set_mode(conv_mode(mode)).unwrap();
            println!("{}", MSG_DONE.green());
        }
        Some(("nasa_image", sub_matches)) => {
            let q = sub_matches
                .get_one::<String>("query")
                .map(|s| s.as_str())
                .unwrap_or("");
            let center = sub_matches
                .get_one::<String>("center")
                .map(|s| s.as_str())
                .unwrap_or("");
            let location = sub_matches
                .get_one::<String>("location")
                .map(|s| s.as_str())
                .unwrap_or("");
            let nasa_id = sub_matches
                .get_one::<String>("nasa_id")
                .map(|s| s.as_str())
                .unwrap_or("");
            let photographer = sub_matches
                .get_one::<String>("photographer")
                .map(|s| s.as_str())
                .unwrap_or("");
            let title = sub_matches
                .get_one::<String>("title")
                .map(|s| s.as_str())
                .unwrap_or("");
            let year_start = sub_matches
                .get_one::<String>("year_start")
                .map(|s| s.as_str())
                .unwrap_or("1900");
            let (est_year, _, _) = get_today_est();
            let est_year_str = est_year.to_string();
            let year_end = sub_matches
                .get_one::<String>("year_end")
                .map(|s| s.as_str())
                .unwrap_or(&est_year_str);

            let nasa_image = get_nasa_image(
                q,
                center,
                location,
                nasa_id,
                photographer,
                title,
                year_start,
                year_end,
            );
            println!("{}", nasa_image);
            println!("{}", MSG_CHANGING.yellow());
            wallpaper::set_from_url(&nasa_image.url).unwrap();
            wallpaper::set_mode(conv_mode(mode)).unwrap();
            println!("{}", MSG_DONE.green());
        }
        Some(("license", _)) => {
            print_license();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    const ASSET: &str = "https://assets.science.nasa.gov/dynamicimage/assets/science/Moon%20Trail.jpg?w=1824&h=1037&fit=crop&crop=faces%2Cfocalpoint";
    const ARTICLE: &str = "https://science.nasa.gov/apod/example/";

    fn payload(date: &str, media: &str, image: Option<&str>) -> String {
        let mut value = json::object! {
            date: date,
            media_type: media,
            title: "Moon &amp; <em>Stars</em>",
            explanation: "<p><strong>Explanation:</strong> First &amp; &#9733;.</p><p>Second<br>Third <a href=\"https://example.com/\">link</a>.</p>",
            copyright: null,
            credit: "<p>Credit: A &amp; B</p>",
            url: ARTICLE,
        };
        if let Some(image) = image {
            value["hdurl"] = image.into();
        }
        value.dump()
    }

    fn serve(status: &str, body: String) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/apod", listener.local_addr().unwrap());
        let status = status.to_owned();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
            }
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            String::from_utf8(request).unwrap().lines().next().unwrap().to_owned()
        });
        (base, handle)
    }

    fn fetch(body: String) -> Apod {
        let (base, request) = serve("200 OK", body);
        let apod = get_apod_from("1999-03-27", &base).unwrap();
        assert_eq!(request.join().unwrap(), "GET /apod/990327 HTTP/1.1");
        apod
    }

    #[test]
    fn calendar_dates_reach_the_expected_single_date_routes() {
        for (date, route) in [
            ("1999-12-31", "991231"),
            ("2000-01-01", "000101"),
            ("2000-02-29", "000229"),
            ("2024-02-29", "240229"),
            ("1900-03-01", "000301"),
        ] {
            let (base, request) = serve("200 OK", payload(date, "image", Some(ASSET)));
            get_apod_from(date, &base).unwrap();
            assert_eq!(request.join().unwrap(), format!("GET /apod/{route} HTTP/1.1"));
        }
    }

    #[test]
    fn invalid_calendar_dates_fail_without_any_http_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        for date in ["1999-02-30", "1900-02-29", "2023-02-29", "2024-13-01", "2024-00-01", "2024-01-00", "2024-1-01", "24-01-01", "2024-01-01extra"] {
            assert!(get_apod_from(date, &base).is_err(), "{date}");
        }
        assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    }

    #[test]
    fn full_date_identity_rejects_wrong_day_and_century() {
        for returned in ["1999-03-28", "2099-03-27"] {
            let (base, request) = serve("200 OK", payload(returned, "image", Some(ASSET)));
            let error = get_apod_from("1999-03-27", &base).err().unwrap().to_string();
            assert!(error.contains("1999-03-27") && error.contains(returned));
            request.join().unwrap();
        }
    }

    #[test]
    fn http_failures_keep_status_and_original_response_details() {
        let (base, request) = serve("503 Service Unavailable", "Upstream temporarily unavailable".to_owned());
        let error = get_apod_from("1999-03-27", &base).err().unwrap().to_string();
        assert!(error.contains("503 Service Unavailable") && error.contains("Upstream temporarily unavailable"), "{error}");
        request.join().unwrap();
        let (base, request) = serve("200 OK", "not valid JSON".to_owned());
        assert!(get_apod_from("1999-03-27", &base).is_err());
        request.join().unwrap();
    }

    #[test]
    fn missing_entries_report_a_plain_message() {
        let (base, request) = serve("404 Not Found", r#"{"code":"apod_basic_not_found","message":"APOD not found."}"#.to_owned());
        let error = get_apod_from("1999-03-27", &base).err().unwrap().to_string();
        assert_eq!(error, "No APOD has been published for 1999-03-27 yet.");
        request.join().unwrap();
    }

    #[test]
    fn default_selection_uses_exact_asset_not_article() {
        let apod = fetch(payload("1999-03-27", "image", Some(ASSET)));
        assert_eq!(apod_image_url(&apod, false).unwrap().as_deref(), Some(ASSET));
    }

    #[test]
    fn nonimage_entries_never_install_featured_stills() {
        for media in ["video", "iframe"] {
            for image in [None, Some(ASSET)] {
                let apod = fetch(payload("1999-03-27", media, image));
                assert!(apod_image_url(&apod, true).unwrap().is_none());
            }
        }
    }

    #[test]
    fn missing_null_and_empty_image_assets_fail_with_article_context() {
        let absent = payload("1999-03-27", "image", None);
        let mut null = json::parse(&absent).unwrap();
        null["hdurl"] = json::Null;
        for body in [absent, null.dump(), payload("1999-03-27", "image", Some("")), payload("1999-03-27", "image", Some("  "))] {
            let apod = fetch(body);
            let error = apod_image_url(&apod, false).unwrap_err().to_string();
            assert!(error.contains(ARTICLE));
        }
    }

    #[test]
    fn low_renditions_cap_both_families_preserving_encoded_paths_and_parameters() {
        for prefix in ["dynamicimage/assets", "content/dam"] {
            let image = format!("https://assets.science.nasa.gov/{prefix}/science/Moon%20Trail%2BStars.jpg?w=1824&h=1037&fit=crop&crop=faces%2Cfocalpoint&token=a%2Bb");
            let apod = fetch(payload("1999-03-27", "image", Some(&image)));
            let selected = apod_image_url(&apod, true).unwrap().unwrap();
            let url = reqwest::Url::parse(&selected).unwrap();
            assert_eq!(url.path(), "/dynamicimage/assets/science/Moon%20Trail%2BStars.jpg");
            let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(pairs.get("w").unwrap(), "1280");
            assert_eq!(pairs.get("h").unwrap(), "1037");
            assert_eq!(pairs.get("fit").unwrap(), "clip");
            assert_eq!(pairs.get("crop").unwrap(), "faces,focalpoint");
            assert_eq!(pairs.get("token").unwrap(), "a+b");
            assert_eq!(apod_image_url(&apod, false).unwrap().unwrap(), image);
        }
        for (image, width, height) in [
            ("https://assets.science.nasa.gov/dynamicimage/assets/small.png?w=594&h=516&fit=clip", "594", "516"),
            ("https://assets.science.nasa.gov/content/dam/tall.jpg?w=900&h=3000", "900", "1280"),
        ] {
            let apod = fetch(payload("1999-03-27", "image", Some(image)));
            let selected = apod_image_url(&apod, true).unwrap().unwrap();
            let url = reqwest::Url::parse(&selected).unwrap();
            let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(pairs.get("w").unwrap(), width);
            assert_eq!(pairs.get("h").unwrap(), height);
        }
    }

    #[test]
    fn unsupported_low_renditions_fail_instead_of_installing_full_size() {
        for image in [
            "https://example.com/image.jpg?w=2000&h=2000",
            "http://assets.science.nasa.gov/content/dam/image.jpg?w=2000&h=2000",
            "https://assets.science.nasa.gov/other/image.jpg?w=2000&h=2000",
            "https://assets.science.nasa.gov/content/dam/",
            "https://assets.science.nasa.gov/content/dam/image.jpg",
            "https://assets.science.nasa.gov/content/dam/image.jpg?w=2000",
            "https://assets.science.nasa.gov/content/dam/image.jpg?w=0&h=2000",
            "https://assets.science.nasa.gov/content/dam/image.jpg?w=-1&h=2000",
            "https://assets.science.nasa.gov/content/dam/image.jpg?w=1.5&h=2000",
            "https://assets.science.nasa.gov/content/dam/image.jpg?w=abc&h=2000",
            "https://assets.science.nasa.gov/content/dam/image.jpg?w=2000&h=",
            "https://assets.science.nasa.gov/content/dam/image.jpg?w=4294967296&h=2000",
            "https://assets.science.nasa.gov/content/dam/image.jpg?w=2000&w=900&h=2000",
        ] {
            let apod = fetch(payload("1999-03-27", "image", Some(image)));
            assert!(apod_image_url(&apod, true).is_err(), "{image}");
            assert_eq!(apod_image_url(&apod, false).unwrap().unwrap(), image);
        }
    }

    #[test]
    fn metadata_is_readable_with_entities_paragraphs_and_attribution_fallback() {
        let apod = fetch(payload("1999-03-27", "image", Some(ASSET)));
        assert_eq!(apod.title, "Moon & Stars");
        assert!(apod.explanation.contains("First & ★."));
        assert!(!apod.explanation.contains('<') && !apod.explanation.contains('[') && !apod.explanation.contains("https://example.com"));
        assert!(apod.explanation.contains("Third link."));
        assert!(!apod.explanation.starts_with("Explanation:"));
        assert_eq!(apod.copyright.as_deref(), Some("A & B"));
        for copyright in ["", "<p> </p>", "<p>Copyright:</p>", "<p><b>Copyright:</b> C &#169; D</p>"] {
            let mut body = json::parse(&payload("1999-03-27", "image", Some(ASSET))).unwrap();
            body["copyright"] = copyright.into();
            let apod = fetch(body.dump());
            assert_eq!(apod.copyright.as_deref(), Some(if copyright.contains("&#169;") { "C © D" } else { "A & B" }));
        }
        let mut body = json::parse(&payload("1999-03-27", "image", Some(ASSET))).unwrap();
        body["copyright"] = "<b>Image Credit &amp; <a href=\"https://example.com/rights\">Copyright</a>:</b> <a href=\"https://example.com/photographer\">Federico Pelliccia</a>".into();
        assert_eq!(fetch(body.dump()).copyright.as_deref(), Some("Federico Pelliccia"));
        let mut body = json::parse(&payload("1999-03-27", "image", Some(ASSET))).unwrap();
        body.remove("copyright");
        assert_eq!(fetch(body.dump()).copyright.as_deref(), Some("A & B"));
    }

    #[test]
    fn metadata_preserves_paragraph_and_line_break_boundaries() {
        let text = render_apod_text("<p>Alpha</p><p>Beta<br>Gamma</p>").unwrap();
        let paragraphs: Vec<Vec<_>> = text
            .split("\n\n")
            .map(|paragraph| paragraph.lines().collect())
            .collect();
        assert_eq!(paragraphs, [vec!["Alpha"], vec!["Beta", "Gamma"]]);
    }


    #[test]
    fn removed_key_options_are_rejected() {
        for removed in ["--key", "-k"] {
            let error = cli()
                .try_get_matches_from(["nasa-wallpaper", "apod", removed, "obsolete"])
                .unwrap_err();
            assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        }
    }
}
