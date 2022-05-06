use bevy::prelude::*;
use heron::prelude::*;

use tokio::runtime::Runtime;

use futures::{
    sink::SinkExt,
    stream::{SplitSink, StreamExt},
};

// TODO: understand how bevy handles design vs test resolutions
const X_RESOLUTION: f32 = 1920.0;
const Y_RESOLUTION: f32 = 1080.0;

// this is based on the size of the sprite texture
// TODO: figure out cleaner, automatic scaling
const PLAYER_RADIUS: f32 = 128.0;
const ENEMY_HALF_EXTENDS: f32 = 128.0;

// TODO: in general there are a lot of hard-coded values, these need to be sorted out

// TODO: the size of the project warrants better organization, but I don't know the
// best way to organize a Bevy project, this will be interesting to learn and explore!
// I imagine several of the Components, Resources, and Systems here should be organized
// into their own modules and Bevy Plugins

// TODO: handle unwraps, expects, and unhandled Results! This includes
// handling failed or cut connections to Deepgram and/or the microphone.

#[derive(PhysicsLayer)]
enum Layer {
    World,
    Player,
    Enemies,
}

#[derive(Component)]
struct Player;

#[derive(Component)]
struct Enemy;

/// Sets up the game's main camera - a simple 2d orthographic camera.
/// This camera will not move, effectively making this a "single screen" non-scrolling game.
fn setup_camera(mut commands: Commands) {
    // TODO: Figure out how to correctly change the scaling mode.
    commands.spawn_bundle(OrthographicCameraBundle::new_2d());
}

/// Spawns an Entity to represent the Player with Sprite, RigidBody, CollisionShape, and other Components.
/// It adds the "Player" Component which can be used to identify the Player in a system by using "With<Player>".
fn spawn_player(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands
        .spawn_bundle(SpriteBundle {
            // this image happens to be 256x256 pixels in size
            texture: asset_server.load("icon.png"),
            ..default()
        })
        .insert(RigidBody::Dynamic)
        .insert(CollisionShape::Sphere {
            radius: PLAYER_RADIUS,
        })
        .insert(Velocity::from_linear(Vec3::ZERO))
        .insert(Acceleration::from_linear(Vec3::ZERO))
        .insert(PhysicMaterial {
            friction: 1.0,
            density: 10.0,
            ..Default::default()
        })
        .insert(RotationConstraints::lock())
        .insert(
            CollisionLayers::none()
                .with_group(Layer::Player)
                .with_mask(Layer::World)
                .with_mask(Layer::Enemies),
        )
        .insert(Damping::from_linear(0.5))
        .insert(Player);
}

/// Spawns an Entity to represent an Enemy, with Sprite, RigidBody, CollisionShape, and other Components.
/// It adds the "Enemy" Component which can be used to identify the Enemy in a system by using "With<Enemy>".
fn spawn_enemy(mut commands: Commands) {
    // RigidBody::KinematicVelocityBased lets me change the velocity, but not the position
    // (and I would like to be able to change both the position and the velocity as I please)
    // I don't want this enemy to be a Dynamic body though because I don't want it to interact
    // with gravity or be pushable by the Player
    // I imagine RotationConstraints::lock() is totally unnecessary for non-Dynamic bodies
    commands
        .spawn_bundle(SpriteBundle {
            sprite: Sprite {
                color: Color::rgb(0.75, 0.75, 0.75),
                custom_size: Some(Vec2::new(
                    ENEMY_HALF_EXTENDS * 2.0,
                    ENEMY_HALF_EXTENDS * 2.0,
                )),
                ..default()
            },
            transform: Transform::from_xyz(0.0, -Y_RESOLUTION / 3.0, 0.0),
            ..default()
        })
        .insert(RigidBody::KinematicVelocityBased)
        .insert(CollisionShape::Cuboid {
            half_extends: Vec3::new(ENEMY_HALF_EXTENDS, ENEMY_HALF_EXTENDS, ENEMY_HALF_EXTENDS),
            border_radius: None,
        })
        .insert(Velocity::from_linear(-Vec3::X * 100.0))
        .insert(Acceleration::from_linear(Vec3::ZERO))
        .insert(PhysicMaterial {
            friction: 1.0,
            density: 10.0,
            ..Default::default()
        })
        .insert(RotationConstraints::lock())
        .insert(
            CollisionLayers::none()
                .with_group(Layer::Enemies)
                .with_mask(Layer::World)
                .with_mask(Layer::Player),
        )
        .insert(Enemy);
}

/// If the Player has dropped off the bottom of the screen, re-center the Player (vertically),
/// this will represent a "Game Over". If the Player went out of bounds horizontally, wrap
/// the Player around.
fn check_player_out_of_bounds(
    mut query: Query<(&mut Acceleration, &mut Velocity, &mut Transform), With<Player>>,
) {
    for (mut acceleration, mut velocity, mut transform) in query.iter_mut() {
        if transform.translation.y < -(Y_RESOLUTION / 2.0) - PLAYER_RADIUS - f32::EPSILON {
            acceleration.linear.y = 0.0;
            velocity.linear.y = 0.0;
            transform.translation.y = 0.0;
        }
        if transform.translation.x > (X_RESOLUTION / 2.0) + PLAYER_RADIUS + f32::EPSILON {
            transform.translation.x = -(X_RESOLUTION / 2.0) - PLAYER_RADIUS;
        } else if transform.translation.x < -(X_RESOLUTION / 2.0) - PLAYER_RADIUS - f32::EPSILON {
            transform.translation.x = (X_RESOLUTION / 2.0) + PLAYER_RADIUS;
        }
    }
}

/// If an Enemy goes out of bounds (horizontally), make it start moving in the other direction,
/// essentially making the Enemy move back-and-forth indefinitely.
fn check_enemy_out_of_bounds(mut query: Query<(&mut Velocity, &mut Transform), With<Enemy>>) {
    for (mut velocity, transform) in query.iter_mut() {
        if transform.translation.x > (X_RESOLUTION / 2.0) + ENEMY_HALF_EXTENDS + f32::EPSILON {
            velocity.linear.x = -100.0;
        } else if transform.translation.x
            < -(X_RESOLUTION / 2.0) - ENEMY_HALF_EXTENDS - f32::EPSILON
        {
            velocity.linear.x = 100.0;
        }
    }
}

/// Handle keyboard input. So far the only logic implemented is pressing the space bar will cause the Player to jump.
fn keyboard_input(keys: Res<Input<KeyCode>>, mut query: Query<&mut Velocity, With<Player>>) {
    if keys.just_released(KeyCode::Space) {
        for mut velocity in query.iter_mut() {
            velocity.linear.y += 400.0;
        }
    }
}

/// A helper function for converting f32 PCM samples to i16 (linear16) samples.
/// Deepgram currently does not support f32 PCM.
fn f32_to_i16(sample: f32) -> i16 {
    let sample = sample * 32768.0;

    // This is a saturating cast. For more details, see:
    // <https://doc.rust-lang.org/reference/expressions/operator-expr.html#numeric-cast>.
    sample as i16
}

/// This async function must be executed in an async runtime, and it will return a websocket handle
/// to Deepgram, which can be used to send and receive messages, although sending and receiving must
/// also be executed in an async runtime.
async fn connect_to_deepgram(
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let api_key = std::env::var("DEEPGRAM_API_KEY").expect("Deepgram API Key is required.");

    // prepare the connection request with the api key authentication
    // TODO: don't hardcode the encoding, sample rate, or number of channels
    let request = http::Request::builder()
        .method(http::Method::GET)
        .uri("wss://api.deepgram.com/v1/listen?encoding=linear16&sample_rate=44100&channels=1")
        .header("Authorization", format!("Token {}", api_key))
        .body(())
        .expect("Failed to build a connection request to Deepgram.");

    // actually finally connect to deepgram
    // we do this using the prepared http request so that we can get the auth header in there
    let (deepgram_socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("Failed to connect to Deepgram.");

    deepgram_socket
}

/// We will have one handle for the microphone as a global resource.
struct MicrophoneReceiver {
    rx: crossbeam_channel::Receiver<Vec<f32>>,
}

impl FromWorld for MicrophoneReceiver {
    fn from_world(_world: &mut World) -> Self {
        let (audio_sender, audio_receiver) = crossbeam_channel::unbounded();

        connect_to_microphone(audio_sender);

        MicrophoneReceiver { rx: audio_receiver }
    }
}

/// We will pass around an Arc'd Tokio Runtime as a global resourse to
/// be used when executing async tasks.
struct AsyncRuntime {
    rt: std::sync::Arc<Runtime>,
}

impl FromWorld for AsyncRuntime {
    fn from_world(_world: &mut World) -> Self {
        AsyncRuntime {
            rt: std::sync::Arc::new(Runtime::new().unwrap()),
        }
    }
}

/// We will have a single handle for a Deepgram websocket connection as a global resource.
/// This DeepgramWebsocket object/resource will contain a `tx` for sending websocket messages
/// to Deepgram, and an `rx` for handling websocket messages received from Deepgram. Note that
/// the `tx` must be used in an async runtime, while the `rx` can be used in any runtime.
struct DeepgramWebsocket {
    tx: SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tungstenite::Message,
    >,
    rx: crossbeam_channel::Receiver<tungstenite::Message>,
}

impl FromWorld for DeepgramWebsocket {
    fn from_world(world: &mut World) -> Self {
        let rt = world.get_resource::<AsyncRuntime>().unwrap();
        let rt = rt.rt.clone();

        let ws = rt.block_on(async { connect_to_deepgram().await });

        let (tx, rx) = crossbeam_channel::unbounded();

        let (ws_tx, mut ws_rx) = ws.split();

        // Here we spawn an indefinite async task which receives websocket messages from Deepgram and pipes
        // them into a crossbeam channel, allowing the main synchronous Bevy runtime to access them when
        // needed (e.g. once per frame in the game loop).
        rt.spawn(async move {
            while let Some(Ok(message)) = ws_rx.next().await {
                let _ = tx.send(message);
            }
        });

        DeepgramWebsocket { tx: ws_tx, rx }
    }
}

/// This uses the portaudio crate to get a connection with your computer's default audio input device (microphone).
/// It takes the sender half of a channel as an input because this function will spawn a thread which pipes audio
/// from the microphone to the receiving half of the channel. An example usage is:
/// ```
/// let (tx, rx) = crossbeam_channel::unbounded();
/// connect_to_microphone(tx);
/// while let Ok(audio) = rx.try_recv() {
///     // do something with the audio
/// }
/// ```
/// This is based on the following tutorial: https://dev.to/maniflames/audio-visualization-with-rust-4nhg
fn connect_to_microphone(tx: crossbeam_channel::Sender<Vec<f32>>) {
    let port_audio = portaudio::PortAudio::new().expect("Initializing PortAudio failed.");
    let mic_index = port_audio
        .default_input_device()
        .expect("Failed to get default input device.");
    let mic_info = port_audio
        .device_info(mic_index)
        .expect("Failed to get microphone info.");
    let input_params = portaudio::StreamParameters::<f32>::new(
        mic_index,
        1,
        true,
        mic_info.default_low_input_latency,
    );

    let input_settings =
        portaudio::InputStreamSettings::new(input_params, mic_info.default_sample_rate, 256);

    let (audio_sender, audio_receiver) = crossbeam_channel::unbounded();

    let audio_callback =
        move |portaudio::InputStreamCallbackArgs { buffer, .. }| match audio_sender.send(buffer) {
            Ok(_) => portaudio::Continue,
            Err(_) => portaudio::Complete,
        };

    let mut audio_stream = port_audio
        .open_non_blocking_stream(input_settings, audio_callback)
        .expect("Failed to create audio stream.");
    audio_stream.start().expect("Failed to start audio stream.");

    // Here we spawn an indefinite synchronous task in its own thread which receives audio from
    // the microphone and pipes it into a crossbeam channel allowing Bevy to access the audio
    // when needed (e.g. once per frame in the game loop) via the receiving half of the channel.
    std::thread::spawn(move || {
        while audio_stream.is_active().unwrap() {
            while let Ok(audio_buffer) = audio_receiver.try_recv() {
                let _ = tx.send(audio_buffer.to_owned());
            }
        }
    });
}

/// This is probably the most complex system this game will have. It requires the global resources
/// which handle receiving audio from the microphone, sending and receiving websocket messages from
/// Deepgram, and the async runtime needed to execute the sending of audio to Deepgram. It additionally
/// requires the Player entity (or entities one day, I guess).
///
/// The logic here is:
/// 1. synchronously try to grab all of the audio from the microphone since the last game loop iteration
/// 2. convert that audio from f32 samples to i16 samples to a buffer of u8
/// 3. send the audio to Deepgram via a blocking send using the async runtime
/// 4. synchronously try to grab all of the websocket messages from Deepgram since the last game loop iteration
/// 5. if a message/transcript result from Deepgram contains the word "up/down/left/right" make the Player jump
fn control_player_with_deepgram(
    microphone_receiver: Res<MicrophoneReceiver>,
    mut deepgram_websocket: ResMut<DeepgramWebsocket>,
    async_runtime: Res<AsyncRuntime>,
    mut query: Query<&mut Velocity, With<Player>>,
) {
    while let Ok(audio_buffer) = microphone_receiver.rx.try_recv() {
        let mut i16_samples = Vec::new();
        for sample in audio_buffer {
            i16_samples.push(f32_to_i16(sample));
        }

        let buffer: &[u8] = unsafe {
            std::slice::from_raw_parts(i16_samples.as_ptr() as *const u8, i16_samples.len() * 2)
        };

        let rt = async_runtime.rt.clone();

        let _ = rt.block_on(async {
            deepgram_websocket
                .tx
                .send(tungstenite::Message::Binary(buffer.to_vec()))
                .await
        });
    }

    while let Ok(message) = deepgram_websocket.rx.try_recv() {
        if let tungstenite::Message::Text(message) = message {
            if message.contains("up") {
                for mut velocity in query.iter_mut() {
                    velocity.linear.y += 400.0;
                }
            }
            if message.contains("down") {
                for mut velocity in query.iter_mut() {
                    velocity.linear.y -= 400.0;
                }
            }
            if message.contains("left") {
                for mut velocity in query.iter_mut() {
                    velocity.linear.x -= 400.0;
                }
            }
            if message.contains("right") {
                for mut velocity in query.iter_mut() {
                    velocity.linear.x += 400.0;
                }
            }
        }
    }
}

fn main() {
    App::new()
        .insert_resource(WindowDescriptor {
            title: "Bevy Deepgram".to_string(),
            width: X_RESOLUTION,
            height: Y_RESOLUTION,
            ..Default::default()
        })
        .add_plugins(DefaultPlugins)
        .add_plugin(PhysicsPlugin::default())
        .insert_resource(Gravity::from(Vec3::new(0.0, -200.0, 0.0)))
        .add_startup_system(setup_camera)
        .add_startup_system(spawn_player)
        .add_startup_system(spawn_enemy)
        .add_system(check_player_out_of_bounds)
        .add_system(check_enemy_out_of_bounds)
        .add_system(keyboard_input)
        .init_resource::<AsyncRuntime>()
        .init_resource::<MicrophoneReceiver>()
        .init_resource::<DeepgramWebsocket>()
        .add_system(control_player_with_deepgram)
        .run();
}
