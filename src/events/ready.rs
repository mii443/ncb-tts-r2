use serenity::{
    model::prelude::{command::Command, Ready},
    prelude::Context,
};

pub async fn ready(ctx: Context, ready: Ready) {
    println!("{} is connected!", ready.user.name);

    let _ = Command::set_global_application_commands(&ctx.http, |commands| {
        commands
            .create_application_command(|command| command.name("stop").description("Stop tts"))
            .create_application_command(|command| {
                command
                    .name("setup")
                    .description("Setup tts")
                    .create_option(|o| {
                        o.name("mode")
                            .description("TTS channel")
                            .add_string_choice("Text Channel", "TEXT_CHANNEL")
                            .add_string_choice("New Thread", "NEW_THREAD")
                            .add_string_choice("Voice Channel", "VOICE_CHANNEL")
                            .kind(serenity::model::prelude::command::CommandOptionType::String)
                            .required(false)
                    })
            })
            .create_application_command(|command| command.name("config").description("Config"))
    })
    .await;
}
