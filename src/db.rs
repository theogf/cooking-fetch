use rusqlite;
use rusqlite::{Connection, Row};
use serde_json;
use std::fs;
use std::fs::create_dir_all;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::sync::Mutex;

#[path = "recipe.rs"]
pub mod recipe;
use recipe::Recipe;

pub fn fill_db(conn: &Mutex<Connection>) {
    match conn.lock().unwrap().execute(
        "CREATE TABLE recipes (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            page_start INTEGER,
            page_end INTEGER,
            has_picture BOOL
        )",
        (),
    ) {
        Ok(_) => (),
        Err(e) => panic!("Failed to build the SQLite DB errored with {}", e),
    };

    let csv_file = Path::new("assets/index.json");

    let json_str = match fs::read_to_string(csv_file) {
        Ok(content) => content,
        Err(e) => panic!("Failed to open index.json with error {}", e),
    };
    let data: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    if let Some(arr) = data.as_array() {
        for obj in arr {
            let recipe = if let Some(item) = obj.as_object() {
                Recipe {
                    id: None,
                    name: {
                        match item.get("name") {
                            Some(name) => {
                                if let Some(name) = name.as_str() {
                                    name.to_string()
                                } else {
                                    panic!("Wrong format for name");
                                }
                            }
                            None => panic!("Name field is missing"),
                        }
                    },
                    page_start: {
                        match item.get("start") {
                            Some(val) => {
                                if let Some(val) = val.as_number() {
                                    val.as_i64().unwrap() as i32
                                } else {
                                    panic!("Wrong format for start");
                                }
                            }
                            None => panic!("Start field is missing"),
                        }
                    },
                    page_end: {
                        match item.get("end") {
                            Some(val) => {
                                if let Some(val) = val.as_number() {
                                    val.as_i64().unwrap() as i32
                                } else {
                                    panic!("Wrong format for end");
                                }
                            }
                            None => panic!("End field is missing"),
                        }
                    },
                    has_picture: match item.get("has_picture") {
                        Some(val) => val.as_bool().unwrap(),
                        None => true,
                    },
                }
            } else {
                log::error!("Wrongly set json item {:?}", obj);
                continue;
            };
            log::debug!("Adding {:?} to db", recipe);
            match conn.lock().unwrap().execute(
                        "INSERT INTO recipes (name, page_start, page_end, has_picture) VALUES (?1, ?2, ?3, ?4)",
                        (recipe.name, recipe.page_start, recipe.page_end, recipe.has_picture),
                    ) {
                        Ok(_) => (),
                        Err(_) => panic!("Failed to insert elements"),
                    };
            // println!("{:?}", recipe);
        }
    } else {
        panic!("Could not parse the content as an array.");
    };
    println!("Finished building the DB");
}

pub fn row_to_recipe(row: &Row) -> Result<Recipe, rusqlite::Error> {
    Ok(recipe::Recipe {
        id: match row.get(0) {
            Ok(id) => Some(id),
            Err(e) => return Err(e),
        },
        name: match row.get(1) {
            Ok(name) => name,
            Err(e) => return Err(e),
        },
        page_start: match row.get(2) {
            Ok(start) => start,
            Err(e) => return Err(e),
        },
        page_end: match row.get(3) {
            Ok(end) => end,
            Err(e) => return Err(e),
        },
        has_picture: match row.get(4) {
            Ok(has) => has,
            Err(e) => return Err(e),
        },
    })
}

fn pdf_path(recipe: Recipe) -> PathBuf {
    let dir_path = Path::new("/tmp/cooking-fetch/pdfs");
    if !dir_path.is_dir() {
        match create_dir_all(dir_path) {
            Ok(_) => (),
            Err(e) => eprintln!("Could not build pdf directory: {}", e),
        };
    }

    let file_name = format!("{}.pdf", recipe.name);
    dir_path.join(file_name)
}

pub fn fetch_or_build_pdf(recipe: Recipe) -> PathBuf {
    let path = pdf_path(recipe.clone());
    // TODO: Handle this directly in rust with either https://github.com/pdf-rs/pdf or https://github.com/J-F-Liu/lopdf
    let pdf_result = process::Command::new("pdftk")
        .arg("assets/book.pdf")
        .arg("cat")
        .arg(format!("{}-{}", recipe.page_start, recipe.page_end))
        .arg("output")
        .arg(path.clone())
        .output();
    match pdf_result {
        Ok(output) => {
            if output.status.success() {
                log::debug!("PDF extracted to {:?}", output);
            } else {
                panic!("Error: {}", String::from_utf8_lossy(&output.stderr));
            }
        }
        Err(e) => eprintln!("Failed to run pdftk: {}", e),
    }
    path
}

pub fn fetch_or_build_image(recipe: Recipe) -> PathBuf {
    let pdf = fetch_or_build_pdf(recipe.clone());
    let dir_images = Path::new("/tmp/cooking-fetch/images");
    if !dir_images.is_dir() {
        match create_dir_all(dir_images) {
            Ok(_) => (),
            Err(e) => eprintln!("Could not build images directory: {}", e),
        };
    }

    let base_image_name = dir_images.join(recipe.clone().name.to_string());
    let img_result = process::Command::new("pdfimages")
        .arg("-png")
        .arg("-print-filenames")
        .arg(pdf)
        .arg(base_image_name)
        .output();
    let img_output = match img_result {
        Ok(output) => {
            if output.status.success() {
                log::debug!("Imgs extracted to {:?}", output);
                String::from_utf8(output.stdout)
                    .unwrap()
                    .trim()
                    .split('\n')
                    .into_iter()
                    .last()
                    .unwrap()
                    .to_string()
            } else {
                panic!("Error: {}", String::from_utf8_lossy(&output.stderr));
            }
        }
        Err(e) => panic!("Failed to run pdfimages: {}", e),
    };
    println!("Fetching image {}", img_output);
    PathBuf::from(img_output)
    // dir_images.join(format!("{}-000.png", recipe.name))
}

pub fn fetch_random_recipe(conn: &Mutex<Connection>, prev_ids: &Vec<i32>) -> Option<Recipe> {
    let db = conn.lock().unwrap();
    let query = if prev_ids.is_empty() {
        "SELECT * FROM recipes ORDER BY RANDOM() LIMIT 1".to_string()
    } else {
        let keys = prev_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "SELECT * FROM recipes WHERE id NOT IN ({}) ORDER BY RANDOM() LIMIT 1",
            keys
        )
    };
    let mut stmt = match db.prepare(query.as_str()) {
        Ok(stmt) => stmt,
        Err(e) => panic!("Failed to pick a random row from the DB with error {}", e),
    };
    let recipe = match stmt.query_row((), row_to_recipe) {
        Ok(recipe) => Some(recipe),
        Err(e) => match e {
            rusqlite::Error::QueryReturnedNoRows => None,
            _ => panic!("Failed to infer the DB row, with error {}", e),
        },
    };
    log::debug!("Fetched random recipe: {:?}", recipe);
    recipe
}

pub fn fetch_recipe_from_id(conn: &Mutex<Connection>, id: &i32) -> Recipe {
    let db = conn.lock().unwrap();
    let mut stmt = match db
        .prepare("SELECT id, name, page_start, page_end, has_picture FROM recipes WHERE id = ?1")
    {
        Ok(stmt) => stmt,
        Err(e) => panic!(
            "Failed to pick a the row from the DB associated with id {} with error {}",
            id, e
        ),
    };
    match stmt.query_row((id,), row_to_recipe) {
        Ok(recipe) => recipe,
        Err(e) => panic!("Failed to infer the DB row, with error {}", e),
    }
}
