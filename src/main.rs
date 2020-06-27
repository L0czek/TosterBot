use serenity::model::id::UserId;
use std::time::Duration;
use std::thread;
use serenity::model::id::ChannelId;
use serenity::http::Http;
use std::sync::Arc;
use rand::Rng;
use chrono;
use serenity::utils::MessageBuilder;
use json::object;
use serenity::client::Client;
use serenity::prelude::*;
use serenity::model::channel::Message;
use serenity::prelude::{EventHandler, Context};
use std::str::FromStr;
use std::error::Error;
use std::io::{Read, Write};
use std::fs::File;
extern crate regex;

use regex::Regex;
#[macro_use] extern crate lazy_static;

const DIRTY_THRESHOLD: u64 = 10 * 60;
const TOASTING_LOW_THRESHOLD: u64 = 60;
const TOASTING_HIGH_THRESHOLD: u64 = 120;
const SMOKING_GOOD_TOAST_CHANCES: f32 = 0.1;
const MAX_ON_TIME: u64 = 360;

type ResponseFunction = fn (&mut Toster, &Context, &Message) -> Result<(), Box<dyn Error>>;

struct Action {
    keywords: Vec<&'static str>,
    function: ResponseFunction
}

struct ChannelCtx {
    channel_id: ChannelId,
    http_ctx: Arc<Http>
}

struct Toster {
    state_file_path: String, 
    start_time: u64,
    toster_dirty: u64,
    is_running: bool,
    current_user: UserId,

    action_table: Vec<Action>, 
    channel_ctx: Option<ChannelCtx>
}

impl Toster {
    
    fn turn_on(&mut self, ctx: &Context, msg: &Message) -> Result<(), Box<dyn Error>> {
        if self.is_running {
            self.respond_to_author(ctx, msg, "Toster jest już włączony!")?; 
        } else {
            self.is_running = true;
            self.current_user = msg.author.id;
            self.start_time = chrono::offset::Local::now().timestamp() as u64;
            self.respond_to_author(ctx, msg, "Włączam toster")?;
        }   
        self.channel_ctx = Some(ChannelCtx{ channel_id: msg.channel_id, http_ctx: Arc::clone(&ctx.http) });
        self.save_state()?;

        Ok(())
    }

    fn turn_off(&mut self, ctx: &Context, msg: &Message) -> Result<(), Box<dyn Error>> {
        if self.is_running {
            let backing_time: u64 = (chrono::offset::Local::now().timestamp() - self.start_time as i64) as u64;
            
            self.toster_dirty += backing_time;
            self.is_running = false;
            self.save_state()?;
            
            let response_first_part = if self.current_user != msg.author.id {
                "Ukradłeś koledze"
            } else {
                "Wypiekłeś"
            };
            
            let mut rng = rand::thread_rng();
            let (toast_quality, toast_img_path) = if backing_time < TOASTING_LOW_THRESHOLD {
                ("niedopieczonego tosta :( weź się lepiej postaraj!", "tost_slaby.jpg")
            } else if backing_time < TOASTING_HIGH_THRESHOLD {
                if self.toster_dirty > DIRTY_THRESHOLD {
                    ("idealnego tosta :) ale toster był brudny....", "tost_dobry.gif")
                } else {
                    ("idealnego tosta :) przy jeszcze czystym tosterze!", "tost_dobry.gif")
                }
            } else if rng.gen::<f32>() < SMOKING_GOOD_TOAST_CHANCES {
                ("smoking GOOD tosta!", "tost_smoking_good.jpg")
            } else {
                ("spalonego tosta!", "tost_spalony.jpg")
            };
            
            let file = File::open(&toast_img_path)?;

            msg.channel_id.send_files(&ctx.http, vec![(&file, toast_img_path)], move |m| {
                m.content(
                    MessageBuilder::new()
                        .mention(&msg.author.id)
                        .push(" ")
                        .push(format!("{} {}", response_first_part, toast_quality))
                        .build()
                )
            })?;
            
        } else {
            self.respond_to_author(ctx, msg, "Toster nie jest włączony!")?;
        }

        Ok(())
    }

    fn is_dirty(&mut self, ctx: &Context, msg: &Message) -> Result<(), Box<dyn Error>> {
        if self.toster_dirty > DIRTY_THRESHOLD {
            self.respond_to_author(ctx, msg, "Toster jest brudny!")?;
        } else if self.toster_dirty > 0 {
            self.respond_to_author(ctx, msg, "Toster jest jeszcze czysty ?!")?;
        } else {
            self.respond_to_author(ctx, msg, "Toster jest idealnie czysty.")?;
        }
        Ok(())
    }

    fn clean_up(&mut self, ctx: &Context, msg: &Message) -> Result<(), Box<dyn Error>> {
        if self.is_running {
            self.respond_to_author(ctx, msg, "Nie możesz czyścić tostera jak jest włączony!")?;
        } else {
            self.toster_dirty = 0;
            self.save_state()?;
            
            let file = File::open("toster_czyszczenie.gif")?;
            msg.channel_id.send_files(&ctx.http, vec![(&file, "czyszczenie.gif")], move |m|{
                m.content(
                    MessageBuilder::new()
                        .mention(&msg.author.id)
                        .push(" Toster jest idealnie czysty!!!")
                        .build()
                )
            })?;
        }

        Ok(())
    }

    fn is_cheese(&mut self, ctx: &Context, msg: &Message) -> Result<(), Box<dyn Error>> {
        self.respond_to_author(ctx, msg, "Oczywiście że jest! Sera dla uczestników nigdy nie braknie")?;
        Ok(())
    }

    pub fn from_file(path: &str) -> Result<Self, Box<dyn Error>> {
        let action_table = vec![
            Action{ keywords: vec![ "włącz" ], function: Self::turn_on },
            Action{ keywords: vec![ "wyłącz" ], function: Self::turn_off },
            Action{ keywords: vec![ "czy", "jest", "brudny" ], function: Self::is_dirty },
            Action{ keywords: vec![ "czy", "jest", "ser" ], function: Self::is_cheese },
            Action{ keywords: vec![ "umyj" ], function: Self::clean_up },
            Action{ keywords: vec![ "wyczyść" ], function: Self::clean_up }
        ];

        let (start_time, toster_dirty) = if let Ok(mut file) = File::open(path) {
            let mut state = String::new();
            file.read_to_string(&mut state)?;
            let parsed = json::parse(state.as_ref())?;
            (parsed["start_time"].as_u64().unwrap_or(0), parsed["toster_dirty"].as_u64().unwrap_or(0))
        } else {
            (0, 0)
        };
        
        let toster = Toster {
            state_file_path: String::from_str(path)?,
            start_time: start_time,
            toster_dirty: toster_dirty,
            is_running: false,
            current_user: UserId::default(),

            action_table: action_table,
            channel_ctx: None
        };

        Ok(toster)
    }

    fn respond_to_author(&self, ctx: &Context, msg: &Message, response: &str) -> Result<(), Box<dyn Error>> {
        let response = MessageBuilder::new()
            .mention(&msg.author.id)
            .push(" ")
            .push(response)
            .build();
        msg.channel_id.say(&ctx.http, &response)?;
        Ok(())
    }

    pub fn save_state(&self) -> Result<(), Box<dyn Error>> {
        let mut file = File::create(&self.state_file_path)?; 
        let state = object!{
            start_time: self.start_time,
            toster_dirty: self.toster_dirty,
            is_running: self.is_running
        };

        let serialized = state.dump();
        file.write(serialized.as_ref())?;

        Ok(())
    }

    pub fn respond(&mut self, ctx: &Context, msg: Message) {
        let content = msg.content.to_lowercase();
        let response_function = self.action_table.iter()
            .find(|ref action| Self::keywords_match(&action.keywords, &content))
            .map(|ref action| action.function)
            .unwrap_or(Self::respond_bad_command);
        
        if let Err(what) = response_function(self, &ctx, &msg) {
            eprintln!("Got error {}", what)
        }
    }

    fn keywords_match(keywords: &Vec<&'static str>, text: &String) -> bool {
        lazy_static![
            static ref RE: Regex = Regex::new(r"\b+").unwrap();
        ];
        
        let mut words = RE.split(text);
        keywords.iter().all(|keyword| words.any(|ref word| word == keyword))
    }

    fn respond_bad_command(&mut self, ctx: &Context, msg: &Message) -> Result<(), Box<dyn Error>> {
        msg.channel_id.say(&ctx.http, "beep boop, jak będziesz źle obsługiwał toster to wywalisz korki")?;
        Ok(())
    }

    pub fn check_on_time(&mut self) {
        let on_time = chrono::offset::Local::now().timestamp() as u64 - self.start_time;
        if self.is_running && on_time >= MAX_ON_TIME {
            if let Err(what) = self.kill() {
                eprintln!("Got error {}", what);
            }  
        } 
    }

    fn kill(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(ctx) = &self.channel_ctx {
            ctx.channel_id.say(&ctx.http_ctx, 
                MessageBuilder::new()
                    .mention(&self.current_user)
                    .push(" zostawiłeś włączony toster żeby się spalił jeszcze pare razy i dostaniesz bana na tosty! (jak ktoś to zaimplementuje...)")
                    .build()
                )?;
        }
        self.is_running = false;
        self.save_state()?;

        Ok(())
    }
}

impl TypeMapKey for Toster { type Value = Arc<RwLock<Self>>; }

struct Handler;

impl EventHandler for Handler {
    fn message(&self, ctx: Context, msg: Message) {
        eprintln!("Got new message {}", msg.content);
        let cache = ctx.cache.read();

        let mut data = ctx.data.write();
        if let Some(toster) = data.get_mut::<Toster>() {
            if msg.mentions_user_id(&cache.user.id) {
                toster.write().respond(&ctx, msg);
            }
        }
    }  
}

fn main() -> Result<(), Box<dyn Error>> {
    let token = std::env::var("DISCORD_TOKEN")?;
    let mut client = Client::new(token, Handler)?;
    let toster = Arc::new(RwLock::new(Toster::from_file("/data/state.json")?));
    {
        let mut data = client.data.write();
        data.insert::<Toster>(Arc::clone(&toster));
    }
    thread::spawn(move || {
        loop {
            toster.write().check_on_time();
            thread::sleep(Duration::from_secs(1));            
        } 
    });
    client.start()?;

    Ok(())
}
