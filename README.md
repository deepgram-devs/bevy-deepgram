# bevy-deepgram

This is essentially a tech-demo showing how one could integrate Deepgram Automatic Speech Recognition (ASR)
and the Bevy game engine. You can control the Bevy icon by saying "up", "down", "left", or "right" to jump
in that direction. There is an "enemy" which moves back and forth and you can collide with. If you fall
off the bottom of the screen, you "die" and are "respawned" in the center of the screen, vertically.

As a tech-demo, this is pretty complete, but there are many TODOs noted in the comments in the code. To run,
set a `DEEPGRAM_API_KEY` environment variable, and simply do:

```
cargo run
```

If things aren't working with the ASR, it may be because your microphone's audio format is different than the
hardcoded values. This demo expects 44100 Hz floating point PCM audio coming from the microphone. Dynamically
choosing the audio format is one of the big TODOs... The game also requires a large 1920x1080 window to work
correctly - reasonable asset and window scaling is another big TODO - in principle, from the Bevy docs, it
looks like this should work like in other engines (like Unity/Godot/etc), but I did not get it working yet.

## A Word On Dependencies.

For Ubuntu, I found that I needed to install the following:

```
sudo apt-get install libasound2-dev libudev-dev
```

For macOS, I found that I needed to install the following:

```
brew install portaudio libsoundio pkg-config
```

With that out of the way, these are the main Rust/Cargo dependencies:

* `bevy`: the game engine
* `heron`: a physics engine and wrapper around `bevy_rapier` providing a simpler API
* `portaudio`: used for microphone input
* `tokio_tungstenite`/`tungstenite`: used to connect to Deepgram via websockets
* `tokio`: used to create an async runtime for the websocket handling

I chose `heron` for the physics engine as it was easier to setup and get working than `bevy_rapier` and felt
much more intuitive. It has limitations for sure, I see no way to directly apply forces and impulses,
but this can be effectively achieved by directly modifying velocities and accelerations. Overall, the
Components `heron` introduces map very well to similar physics engines used in Unity/Godot/etc.

`portaudio` was a clear choice for the microphone input, and there was a nice guide that I followed
to do this part (the guide is linked in the comments actually).

For the websockets, things got a bit tricky. I did not want to introduce an async runtime, and
even got a prototype working without one, but it had severe limitations (namely lag and the potential
to block ASR indefinitely). These limitations stemmed from the fact that doing `socket.read_message()`
is a blocking call. This bugs me as regular channels (and `crossbeam` channels) have a `try_recv()`
method which is not blocking, and having similar functionality for vanilla `tungstenite` websockets
would allow this whole project to work without a need for any async runtime. However, here we are!
