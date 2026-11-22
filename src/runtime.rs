use crate::{
    lang,
    model::{self, Replies},
};
use sp_social::{
    SPY_NODE,
    runtime::{Names, find, id, ignores, spies},
};
use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};
use strawberry_plugin_api::{
    Context, EventHandler, Player, Plugin, PluginMetadata, Result, Server,
    command::{
        Arg, ArgumentType, Command, CommandError, CommandNode, CommandSender, ConsumedArgs,
        StringType,
    },
    commands::CommandHandler,
    common::NamedColor,
    events::{FailureVerdict, HandlerOptions, HandlerPriority, PlayerLeaveEvent},
    events_wit::PlayerLeaveEventData,
    permission::{Permission, PermissionDefault},
    register_plugin, server,
    text::TextComponent,
};

const USE: &str = "msg-lite:use";
type Shared = Arc<Mutex<State>>;

struct State {
    replies: Replies,
    clock: Instant,
}

fn lock(state: &Shared) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A line of a private message: the plain text of the language file with the names and the message put in.
/// Nothing in it is read as a colour code or a component.
fn line(locale: &str, key: &str, args: &[&str]) -> TextComponent {
    TextComponent::text(&lang::text(locale, key, args)).color_named(NamedColor::Gray)
}

/// Sends `text` from `from` to `target`. A target the writer cannot see is answered exactly like a player who
/// is not on the server; a reader who ignores the writer is answered exactly like one who read it.
fn whisper(
    shared: &Shared,
    server: &Server,
    from: &Player,
    target: Player,
    text: &str,
) -> std::result::Result<(), &'static str> {
    let (me, to) = (id(from), id(&target));
    if to == me {
        return Err("self");
    }
    let (to_name, to_locale) = (target.get_name(), target.get_locale());
    if !from.can_see_player(target) {
        return Err("not_found");
    }
    let from_name = from.get_name();
    let ignored = ignores(to, from);
    {
        let mut state = lock(shared);
        let now = u64::try_from(state.clock.elapsed().as_millis()).unwrap_or(u64::MAX);
        if ignored {
            state.replies.wrote(me, to, now);
        } else {
            state.replies.delivered(me, to, now);
        }
    }
    if !ignored && let Some(target) = find(server, to) {
        target.send_system_message(
            line(&to_locale, "from", &[&from_name, text])
                .italic(true)
                .click_suggest_command(&format!("/msg {from_name} ")),
            false,
        );
    }
    from.send_system_message(
        line(&from.get_locale(), "to", &[&to_name, text]).italic(true),
        false,
    );
    for spy in spies() {
        if spy == me || spy == to {
            continue;
        }
        // The permission is asked now, on the staff member as he is: a switch left on does not outlive it.
        if let Some(staff) = find(server, spy)
            && staff.has_permission(SPY_NODE)
        {
            staff.send_system_message(
                line(&staff.get_locale(), "spy", &[&from_name, &to_name, text]),
                false,
            );
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Action {
    Message,
    Reply,
}

struct Run(Shared, Action);

impl Run {
    fn run(
        &self,
        server: &Server,
        player: &Player,
        args: &ConsumedArgs,
    ) -> std::result::Result<(), &'static str> {
        if !player.has_permission(USE) {
            return Err("no_permission");
        }
        let text = match args.get_value("message") {
            Arg::Simple(raw) => model::message(&raw)?,
            _ => return Err("empty"),
        };
        let target = match self.1 {
            Action::Message => {
                let Arg::Simple(raw) = args.get_value("player") else {
                    return Err("not_found");
                };
                server.get_player_by_name(model::name(&raw).ok_or("not_found")?)
            }
            Action::Reply => {
                let partner = { lock(&self.0).replies.partner(id(player)) }.ok_or("no_reply")?;
                find(server, partner)
            }
        };
        whisper(&self.0, server, player, target.ok_or("not_found")?, &text)
    }
}

impl CommandHandler for Run {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> std::result::Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            // The console keeps the game's own whisper, with its selectors.
            if sender.is_console()
                && matches!(self.1, Action::Message)
                && let (Arg::Simple(target), Arg::Simple(message)) =
                    (args.get_value("player"), args.get_value("message"))
            {
                let ran = server.dispatch_command(
                    &format!("minecraft:msg {target} {message}"),
                    server::CommandSender::Console,
                );
                return Ok(i32::from(ran));
            }
            return Err(CommandError::CommandFailed(TextComponent::text(
                &lang::text("en", "players_only", &[]),
            )));
        };
        self.run(&server, &player, &args)
            .map(|()| 1)
            .map_err(|key| {
                CommandError::CommandFailed(
                    TextComponent::text(&lang::text(&player.get_locale(), key, &[]))
                        .color_named(NamedColor::Red),
                )
            })
    }
}

struct Events(Shared);

impl EventHandler<PlayerLeaveEvent> for Events {
    fn handle(&self, _server: Server, event: PlayerLeaveEventData) -> PlayerLeaveEventData {
        lock(&self.0).replies.forget(id(&event.player));
        event
    }
}

struct MsgLite {
    state: Shared,
}

impl Plugin for MsgLite {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                replies: Replies::default(),
                clock: Instant::now(),
            })),
        }
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "msg-lite".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["Strawberry Interactive LLC".into()],
            description: lang::text("en", "description", &[]),
            dependencies: vec![],
            permissions: vec![],
        }
    }

    fn on_load(&self, context: Context) -> Result<()> {
        context.register_permission(&Permission {
            node: USE.into(),
            description: lang::text("en", "permission_use", &[]),
            default: PermissionDefault::Allow,
            children: vec![],
        })?;
        let message =
            || CommandNode::argument("message", &ArgumentType::String(StringType::Greedy));
        // Only the labels a mute of admin-lite stops: a muted player has no way around it here.
        context.register_command(
            Command::new(
                &[
                    "msg".into(),
                    "tell".into(),
                    "w".into(),
                    "m".into(),
                    "whisper".into(),
                ],
                &lang::text("en", "command_msg", &[]),
            )
            .then(
                CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                    .suggest(Names)
                    .then(message().execute(Run(Arc::clone(&self.state), Action::Message))),
            ),
            USE,
        );
        context.register_command(
            Command::new(
                &["r".into(), "reply".into()],
                &lang::text("en", "command_reply", &[]),
            )
            .then(message().execute(Run(Arc::clone(&self.state), Action::Reply))),
            USE,
        );
        // A leave is only watched; a hidden leave line (a cancelled event) is still a leave.
        context.register_event_handler_with_options::<PlayerLeaveEvent, _>(
            Events(Arc::clone(&self.state)),
            HandlerOptions {
                priority: HandlerPriority::Monitor,
                blocking: true,
                ignore_cancelled: false,
                on_failure: FailureVerdict::Allow,
            },
        )?;
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> Result<()> {
        lock(&self.state).replies.clear();
        Ok(())
    }
}

register_plugin!(MsgLite);
