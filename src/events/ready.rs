use serenity::{
    all::{Command, CommandOptionType, CreateCommand, CreateCommandOption}, model::prelude::Ready, prelude::Context
};

pub async fn ready(ctx: Context, ready: Ready) {
    println!("{} is connected!", ready.user.name);

    Command::set_global_commands(&ctx.http, vec![
        CreateCommand::new("stop").description("Stop tts"),
        CreateCommand::new("setup").description("Setup tts").set_options(vec![
            CreateCommandOption::new(CommandOptionType::String, "mode", "TTS channel")
            .add_string_choice("Text Channel", "TEXT_CHANNEL")
            .add_string_choice("New Thread", "NEW_THREAD")
            .add_string_choice("Voice Channel", "VOICE_CHANNEL")
            .required(false)
        ]),
        CreateCommand::new("config").description("Config"),
        CreateCommand::new("skip").description("skip tts message"),
    ]).await.unwrap();
}
