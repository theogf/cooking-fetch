use dotenv::dotenv;
use std::sync::{Arc, Mutex};
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::types::{InputFile, ParseMode};
// use std::env;
// use rand::distributions::weighted::{Weighted, WeightedChoice};
// use rand::distributions::Distribution;
use rusqlite::Connection;
use teloxide::dispatching::*;
use teloxide::{prelude::*, utils::command::BotCommands};

type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
type MyDialogue = Dialogue<State, InMemStorage<State>>;

#[path = "db.rs"]
mod db_utils;

#[derive(BotCommands, Clone)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are supported:"
)]
enum Command {
    #[command(description = "Display this text.")]
    Help,
    #[command(description = "Provide a random recipe.")]
    New,
    #[command(description = "Accept the proposed recipe and send the pdf of the recipe.")]
    Accept,
    #[command(description = "Ask for another proposed recipe.")]
    Next,
}

#[derive(Clone, Default)]
pub enum State {
    #[default]
    Start,
    FindRecipe(Vec<i32>),
}

#[tokio::main]
async fn main() {
    // Load all env variables from .env file.
    dotenv().ok();
    std::env::set_var("RUST_LOG", "debug");
    pretty_env_logger::init();
    log::info!("Starting bot...");

    let bot = Bot::from_env();

    log::info!("Loading database");

    let conn = Mutex::new(match Connection::open_in_memory() {
        Ok(conn) => conn,
        Err(e) => panic!("Failed to open SQLite in memory with error {}", e),
    });

    db_utils::fill_db(&conn);
    let handler = Update::filter_message()
        .filter_command::<Command>()
        .enter_dialogue::<Message, InMemStorage<State>, State>()
        .branch(dptree::case![Command::New].endpoint(start_recipe))
        .branch(dptree::case![Command::Help].endpoint(help))
        .branch(dptree::case![Command::Next].endpoint(send_random_recipe))
        .branch(dptree::case![Command::Accept].endpoint(send_pdf));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![Arc::new(conn), InMemStorage::<State>::new()])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}

async fn help(bot: Bot, msg: Message) -> HandlerResult {
    bot.send_message(msg.chat.id, Command::descriptions().to_string())
        .await
        .unwrap();
    Ok(())
}

async fn start_recipe(
    bot: Bot,
    dialogue: MyDialogue,
    conn: Arc<Mutex<Connection>>,
    msg: Message,
) -> HandlerResult {
    send_random_recipe(bot, dialogue, conn, msg, State::Start).await
}

static SPECIAL_CHARACTERS: [char; 18] = [
    '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
];

fn escape_markdown(str: String) -> String {
    let mut new_str = String::new();
    for c in str.chars() {
        if SPECIAL_CHARACTERS.contains(&c) {
            new_str.push('\\');
        }
        new_str.push(c)
    }
    new_str
}

async fn send_random_recipe(
    bot: Bot,
    dialogue: MyDialogue,
    conn: Arc<Mutex<Connection>>,
    msg: Message,
    state: State,
) -> HandlerResult {
    let mut prev_ids = match state {
        State::Start => Vec::<i32>::new(),
        State::FindRecipe(ids) => ids,
    };
    let random_recipe = match db_utils::fetch_random_recipe(&conn, &prev_ids) {
        Some(recipe) => recipe,
        None => {
            bot.send_message(
                msg.chat.id,
                "You circled over all recipes. You can start over with /recipe",
            )
            .await
            .unwrap();
            return Ok(());
        }
    };

    let text_end = "\n\n/accept to get the pdf\n/next for another recipe";
    if random_recipe.has_picture {
        let img = db_utils::fetch_or_build_image(random_recipe.clone());
        match bot
            .send_photo(msg.chat.id, InputFile::file(img.clone()))
            .caption(format!(
                "*{}*{}",
                escape_markdown(random_recipe.name),
                text_end
            ))
            .parse_mode(ParseMode::MarkdownV2)
            .await
        {
            Ok(_) => (),
            Err(e) => panic!("Failed to send photo stored at {:?}, with error {}", img, e),
        };
    } else {
        bot.send_message(msg.chat.id, format!("*{}*{}", random_recipe.name, text_end))
            .await
            .unwrap();
    }
    prev_ids.push(random_recipe.id.unwrap());
    dialogue.update(State::FindRecipe(prev_ids)).await.unwrap();

    Ok(())
}

async fn send_pdf(
    bot: Bot,
    dialogue: MyDialogue,
    conn: Arc<Mutex<Connection>>,
    msg: Message,
    state: State,
) -> HandlerResult {
    let recipe = match state {
        State::FindRecipe(ids) => db_utils::fetch_recipe_from_id(&conn, ids.last().unwrap()),
        _ => {
            bot.send_message(
                msg.chat.id,
                "To accept you first need to fetch a recipe with /new",
            )
            .await
            .unwrap();
            return Ok(());
        }
    };
    let path = db_utils::fetch_or_build_pdf(recipe);
    bot.send_document(msg.chat.id, InputFile::file(path))
        .await
        .unwrap();
    dialogue.update(State::Start).await.unwrap();
    Ok(())
}
