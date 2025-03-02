use std::net::{SocketAddr, UdpSocket};
use std::io::Result;
use std::os::unix::raw::time_t;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::thread::sleep;
use std::time::{Duration, Instant};
use flatbuffers::{root, FlatBufferBuilder};

#[allow(dead_code, unused_imports)]
#[path = "../schema_generated.rs"]
mod schema_generated;
pub use schema_generated::Player as SchemaPlayer;
use crate::schema_generated::{PlayerCommand, PlayerCommands, Color, PlayerArgs};

const MAX_PLAYERS: usize = 10;
const GRAVITY: f32 = 1.0;
const FRICTION: f32 = 0.8;
const JUMP_CD: f32 = 0.3;
const SCREEN_HEIGHT: usize = 360;
const SCREEN_WIDTH: usize = 640;
const TICK_DURATION: Duration = Duration::from_millis(16);
const SERVER_ADDR: &str = "127.0.0.1:9000";
const PLAYER_SIZE: f32 = 16.0;

struct Vec2 {
    x: f32,
    y: f32,
}

impl Vec2 {
    fn zero() -> Vec2 {
        Vec2 { x: 0.0, y: 0.0 }
    }
}

struct Player {
    ip: SocketAddr,
    pos: Vec2,
    vel: Vec2,
    acc: f32,
    jump_force: f32,
    jump_timer: f32,
    color: Color,
}

impl Player {
    fn new(ip: SocketAddr) -> Player {
        Player {
            ip,
            pos: Vec2::zero(),
            vel: Vec2::zero(),
            acc: 0.75,
            jump_force: 10.0,
            jump_timer: 0.0,
            color: Color::Red,
        }
    }
}

fn main() -> Result<()> {
    let socket = Arc::new(UdpSocket::bind(SERVER_ADDR)?);
    println!("UDP running on {}...", SERVER_ADDR);
    let players: Arc<Mutex<Vec<Player>>> = Arc::new(Mutex::new(Vec::new()));
    let commands: Arc<Mutex<Vec<(SocketAddr, PlayerCommand)>>> = Arc::new(Mutex::new(Vec::new()));

    let tick_players = Arc::clone(&players);
    let tick_commands = Arc::clone(&commands);
    let tick_socket = Arc::clone(&socket);

    thread::spawn(move || {
        loop {
            let start = Instant::now();

            let mut players_guard = tick_players.lock().unwrap();
            let mut commands_guard = tick_commands.lock().unwrap();
            tick(&mut players_guard, &mut commands_guard, &tick_socket);
            drop(players_guard);
            drop(commands_guard);

            let sleep_time = TICK_DURATION.checked_sub(start.elapsed());
            if let Some(sleep_time) = sleep_time {
                sleep(sleep_time)
            }
        }
    });

    loop {
        let mut buf = [0u8; 2048];
        let (amt, src_addr) = socket.recv_from(&mut buf)?;

        let mut commands_guard = commands.lock().unwrap();
        handle_packet(&buf[..amt], src_addr, &mut commands_guard);
        drop(commands_guard)
    }
}

fn tick(players: &mut MutexGuard<Vec<Player>>,
        commands: &mut Vec<(SocketAddr, PlayerCommand)>,
        socket: &UdpSocket) {
    for (addr, cmd) in commands.iter() {
        if let Some(player) = get_player_by_ip(addr, players) {
            match cmd {
                &PlayerCommand::Move_right => handle_move_right(player),
                &PlayerCommand::Move_left => handle_move_left(player),
                &PlayerCommand::Jump => handle_jump(player),
                _ => {}
            }
        } else {
            println!("New player connected: {}", addr);
            players.push(Player::new(*addr));
        }
    }

    physics(players);

    let mut builder = FlatBufferBuilder::with_capacity(2048);
    let players_offsets: Vec<_> = players
        .iter()
        .map(|p| {
            let args = PlayerArgs {
                x: p.pos.x,
                y: p.pos.y,
                color: p.color,
            };
            SchemaPlayer::create(&mut builder, &args)
        })
        .collect();

    let players_vec = builder.create_vector(&players_offsets);
    let players_list = schema_generated::PlayersList::create(
        &mut builder,
        &schema_generated::PlayersListArgs {
            players: Some(players_vec),
        },
    );
    builder.finish(players_list, None);
    let bytes = builder.finished_data();
    for p in players.iter() {
        let _ = socket.send_to(bytes, p.ip);
    }

    commands.clear();
}

fn handle_packet(packet: &[u8], src_addr: SocketAddr, commands: &mut MutexGuard<Vec<(SocketAddr, PlayerCommand)>>) {
    let player_commands = root::<PlayerCommands>(packet).expect("No command received");
    if let Some(cmd_list) = player_commands.commands() {
        for cmd in cmd_list {
            commands.push((src_addr, cmd));
        }
    }
}

fn physics(players: &mut [Player]) {
    for player in players {
        player.pos.x = player.pos.x + player.vel.x;
        player.pos.y = player.pos.y + player.vel.y;
        player.vel.x *= FRICTION;
        player.vel.y += GRAVITY;
        player.jump_timer += 0.16;

        if player.pos.y > SCREEN_HEIGHT as f32 - PLAYER_SIZE {
            player.pos.y = SCREEN_HEIGHT as f32 - PLAYER_SIZE;
            player.vel.y = 0.0;
        }
        if player.pos.y < 0.0 {
            player.pos.y = 0.0;
            player.vel.y = 0.0;
        }
        if player.pos.x > SCREEN_WIDTH as f32 - PLAYER_SIZE {
            player.pos.x = SCREEN_WIDTH as f32 - PLAYER_SIZE;
            player.vel.x = 0.0;
        }
        if player.pos.x < 0.0 {
            player.pos.x = 0.0;
            player.vel.x = 0.0;
        }
    }
}

fn get_player_by_ip<'a>(ip: &SocketAddr, players: &'a mut MutexGuard<Vec<Player>>) -> Option<&'a mut Player> {
    players.iter_mut().find(|p| p.ip == *ip)
}

fn handle_move_right(player: &mut Player) {
    player.vel.x += player.acc;
}

fn handle_move_left(player: &mut Player) {
    player.vel.x -= player.acc;
}

fn handle_jump(player: &mut Player) {
    if player.pos.y >= SCREEN_HEIGHT as f32 - PLAYER_SIZE && player.jump_timer > JUMP_CD {
        player.vel.y -= player.jump_force;
        player.jump_timer = 0.0;
    };
}
