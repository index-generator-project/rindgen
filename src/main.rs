use base64::prelude::*;
use chrono::{DateTime, Local};
use getopts::{Matches, Options};
use include_dir::{include_dir, Dir};
use serde::Serialize;
use std::{collections::HashMap, env, fs::File, io::Write, path::MAIN_SEPARATOR_STR};
use tera::{Context, Tera};
use walkdir::{DirEntry, WalkDir};

#[derive(Serialize)]
struct Product {
    ig: Index,
}

#[derive(Serialize)]
struct Index {
    root: String,
    files: Vec<FileItem>,
    generator: Generator,
}

#[derive(Serialize)]
struct FileItem {
    path: String,
    name: String,
    size: String,
    modified: String,
    mime: String,
    is_dir: bool,
    icon: String,
}

#[derive(Serialize, Clone)]
struct Generator {
    name: String,
    version: String,
    url: String,
}

static TEMPLATE_DIR: Dir = include_dir!("templates");
static ICON_DIR: Dir = include_dir!("icons");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.optflag("V", "version", "Print version infomation and quit.")
        .optopt(
            "t",
            "theme",
            "Select builtin theme to generate html.",
            "[default, default-dark]",
        )
        .optopt("T", "template", "Custom template to generate html.", "PATH")
        .optflag("", "no-recursive", "Do not generate recursively.")
        .optopt("n", "name", "Default output filename.", "NAME")
        .optflag("P", "print", "Whether to print to stdout.")
        .optopt("d", "depth", "Set cutoff depth.", "NUMBER")
        .optopt("r", "root", "Set base root dir.", "PATH")
        .optflag("", "human", "Make size human readable.")
        .optopt("", "iconset", "Choose iconset.", "ICON")
        .optflag("h", "help", "print this help menu");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => {
            panic!("{}", f.to_string())
        }
    };

    if matches.opt_present("h") {
        print_usage(&program, opts);
        return Ok(());
    }

    app(&program, matches, opts)?;

    Ok(())
}

fn app(program: &str, matches: Matches, opts: Options) -> Result<(), Box<dyn std::error::Error>> {
    let version = matches.opt_present("version");
    if version {
        println!(
            "{} {} {}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_HOMEPAGE")
        );
        return Ok(());
    }

    let path = if !matches.free.is_empty() {
        matches.free[0].clone()
    } else {
        print_usage(program, opts);
        return Ok(());
    };

    if !std::path::Path::new(&path).exists() {
        panic!("Path does not exists");
    }

    let no_recursive = matches.opt_present("no-recursive");
    let theme = matches.opt_str("theme").unwrap_or("default".into());
    let name = matches.opt_str("name").unwrap_or("index.html".into());
    let template = matches.opt_str("template");
    let print = matches.opt_present("print");
    let depth = matches.opt_get_default("depth", usize::MAX)?;
    let root = matches
        .opt_str("root")
        .unwrap_or(MAIN_SEPARATOR_STR.to_owned());
    let human = matches.opt_present("human");
    let iconset = matches.opt_str("iconset").unwrap_or("papirus".into());

    if no_recursive {
        generate(
            theme, &path, name, print, 1, root, human, &template, iconset,
        )?;
    } else {
        generate(
            theme, &path, name, print, depth, root, human, &template, iconset,
        )?;
    }
    Ok(())
}

fn generate(
    theme: String,
    path: &str,
    name: String,
    if_print: bool,
    max_depth: usize,
    base: String,
    human: bool,
    template: &Option<String>,
    iconset: String,
) -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_current_dir(path)?;

    let mut map: HashMap<String, Vec<DirEntry>> = HashMap::new();

    for entry in WalkDir::new(".")
        .max_depth(max_depth)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| {
            !matches!(
                e.file_name().to_str(),
                Some("index.html") | Some("images") | Some("favicon.ico")
            )
        })
    {
        let entry = entry?;

        if entry.depth() == 0 {
            continue;
        }

        if let Some(p) = entry.path().parent() {
            let list = map.entry(p.to_str().unwrap().to_string()).or_default();
            list.push(entry);
        }
    }

    let mut tera = match template {
        Some(t) => Tera::new(t)?,
        None => {
            let mut raw = Tera::default();
            raw.add_raw_template(
                "layout.html",
                TEMPLATE_DIR
                    .get_file(theme.clone() + MAIN_SEPARATOR_STR + "layout.html")
                    .unwrap()
                    .contents_utf8()
                    .unwrap(),
            )?;
            raw.add_raw_template(
                "index.html",
                TEMPLATE_DIR
                    .get_file(theme + MAIN_SEPARATOR_STR + "index.html")
                    .unwrap()
                    .contents_utf8()
                    .unwrap(),
            )?;

            raw
        }
    };

    tera.autoescape_on(vec![".html", ".htm"]);

    for (k, v) in map.iter() {
        let mut files = Vec::new();

        for f in v {
            let mime = mime_guess::from_path(f.path())
                .first_raw()
                .unwrap_or("")
                .to_string();
            let is_dir = f.file_type().is_dir();
            let modified: DateTime<Local> = f.metadata()?.modified()?.into();

            let fi = FileItem {
                path: f.path().to_str().unwrap().into(),
                name: f.file_name().to_str().unwrap().into(),
                size: if human {
                    size_fmt(f.metadata()?.len())
                } else {
                    f.metadata()?.len().to_string()
                },
                modified: modified.format("%Y-%m-%d %H:%M:%S").to_string(),
                mime: mime.clone(),
                is_dir,
                icon: get_icon_by_mime(mime.clone(), is_dir, iconset.clone()),
            };
            files.push(fi);
        }

        let html = tera.render(
            "index.html",
            &Context::from_serialize(Product {
                ig: Index {
                    root: base.clone()
                        + k.strip_prefix('.')
                            .unwrap_or("")
                            .strip_prefix('/')
                            .unwrap_or(""),
                    files,
                    generator: Generator {
                        name: env!("CARGO_PKG_NAME").into(),
                        version: env!("CARGO_PKG_VERSION").into(),
                        url: env!("CARGO_PKG_HOMEPAGE").into(),
                    },
                },
            })?,
        )?;

        if if_print {
            println!("{}", html)
        }

        let mut file = File::create(k.to_owned() + MAIN_SEPARATOR_STR + &name)?;
        file.write_all(html.as_bytes())?;
    }

    Ok(())
}

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options] PATH", program);
    print!("{}", opts.usage(&brief));
}

fn size_fmt(len: u64) -> String {
    let mut len = len as f64;

    for unit in ["", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB"] {
        if len.abs() < 1024.0 {
            return if unit.is_empty() {
                format!("{:3.0} {}", len, unit)
            } else {
                format!("{:3.1} {}", len, unit)
            };
        }
        len /= 1024.0
    }
    format!("{:.1} {}", len, "YiB")
}

fn get_icon_by_mime(mime: String, is_dir: bool, iconset: String) -> String {
    let mut mime = mime.clone();

    if is_dir {
        mime = "inode/directory".into();
    }

    let valid_targets = if mime.is_empty() {
        vec!["default".into()]
    } else {
        let segments: Vec<&str> = mime.split('/').collect();
        vec![
            segments[0].to_owned() + MAIN_SEPARATOR_STR + segments[1],
            segments[0].to_owned(),
            "default".into(),
        ]
    };

    for target in valid_targets {
        match ICON_DIR.get_file(iconset.clone() + MAIN_SEPARATOR_STR + &target + ".svg") {
            Some(i) => {
                return BASE64_STANDARD.encode(i.contents());
            }
            None => continue,
        }
    }

    "".into()
}
