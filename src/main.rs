use libp2p::futures::StreamExt;
use libp2p::swarm::SwarmEvent;
use libp2p::{gossipsub, mdns, noise, swarm::NetworkBehaviour, tcp, yamux};
use rand;
use std::{error::Error, time::Duration};
use tokio::{
    io::{self, AsyncBufReadExt},
    select,
};
use tracing_subscriber::EnvFilter;

#[derive(NetworkBehaviour)]
pub struct MyBehaviour {
    pub mdns: mdns::tokio::Behaviour,
    pub gossipsub: gossipsub::Behaviour,
}

#[tokio::main]

async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| {
            // Set a custom gossipsub configuration
            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub::ConfigBuilder::default()
                    .validation_mode(gossipsub::ValidationMode::Strict)
                    .message_id_fn(|_message: &gossipsub::Message| {
                        gossipsub::MessageId::from(rand::random::<u64>().to_string())
                    })
                    .build()?,
            )?;

            let mdns =
                mdns::tokio::Behaviour::new(mdns::Config::default(), key.public().to_peer_id())?;
            Ok(MyBehaviour { gossipsub, mdns })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    let topic = gossipsub::IdentTopic::new("desnet/chat");
    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines();

    loop {
        select! {
          Ok(Some(line)) = stdin.next_line() => {
            if let Err(e) = swarm
                .behaviour_mut().gossipsub
                .publish(topic.clone(), line.as_bytes()) {
                println!("Publish error: {e:?}");
            }
          }
          event = swarm.select_next_some() => match event {
            SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
              for (peer_id, _multiaddr) in list {
                println!("mDNS discovered a new peer: {peer_id}");
                swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
              }
            }
            SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
              for (peer_id, _multiaddr) in list {
                println!("mDNS discover peer has expired: {peer_id}");
                swarm
                  .behaviour_mut()
                  .gossipsub
                  .remove_explicit_peer(&peer_id);
              }
            }
            SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
              propagation_source: peer_id,
              message_id: id,
              message,
            })) => println!(
              "Got message: '{}' with id: {id} from peer: {peer_id}",
              String::from_utf8_lossy(&message.data),
            ),
            SwarmEvent::NewListenAddr { address, .. } => {
              println!("Local node is listening on {address}");
            }
            _ => {}
          }
        }
    }
}
