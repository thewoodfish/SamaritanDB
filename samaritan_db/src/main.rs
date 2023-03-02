#![warn(rust_2018_idioms)]

use tokio::net::TcpListener;
use tokio_stream::StreamExt;
use tokio_util::codec::{Framed, LinesCodec};

use futures::SinkExt;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::sync::{Arc, Mutex};

/// The in-memory database shared amongst all clients.
///
/// This database will be shared via `Arc`, so to mutate the internal map we're
/// going to use a `Mutex` for interior mutability.
struct Database {
    map: Mutex<HashMap<String, String>>,
}

/// The struct that describes behaviour of the database
// struct Config {
//     contract_addr: String
// }

/// Possible requests our clients can send us
enum Request<'a> {
    New { class: &'a str, password: &'a str },
    Get { key: String },
    Set { key: String, value: String },
}

/// Responses to the `Request` commands above
enum Response {
    Single(String),
    Double {
        one: String,
        two: String,
    },
    Triple {
        one: String,
        two: String,
        three: Option<String>,
    },
    Error {
        msg: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Parse the address we're going to run this server on
    // and set up our TCP listener to accept connections.
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());

    let listener = TcpListener::bind(&addr).await?;
    println!("Listening on: {}", addr);

    // Create the shared state of this server that will be shared amongst all
    // clients. We populate the initial database and then create the `Database`
    // structure. Note the usage of `Arc` here which will be used to ensure that
    // each independently spawned client will have a reference to the in-memory
    // database.
    let mut initial_db = HashMap::new();
    initial_db.insert("foo".to_string(), "bar".to_string());
    let db = Arc::new(Database {
        map: Mutex::new(initial_db),
    });

    loop {
        match listener.accept().await {
            Ok((socket, _)) => {
                // After getting a new connection first we see a clone of the database
                // being created, which is creating a new reference for this connected
                // client to use.
                let db = db.clone();

                // Like with other small servers, we'll `spawn` this client to ensure it
                // runs concurrently with all other clients. The `move` keyword is used
                // here to move ownership of our db handle into the async closure.
                tokio::spawn(async move {
                    // Since our protocol is line-based we use `tokio_codecs`'s `LineCodec`
                    // to convert our stream of bytes, `socket`, into a `Stream` of lines
                    // as well as convert our line based responses into a stream of bytes.
                    let mut lines = Framed::new(socket, LinesCodec::new());

                    // Here for every line we get back from the `Framed` decoder,
                    // we parse the request, and if it's valid we generate a response
                    // based on the values in the database.
                    while let Some(result) = lines.next().await {
                        match result {
                            Ok(line) => {
                                let response = handle_request(&line, &db);

                                let response = response.serialize();

                                if let Err(e) = lines.send(response.as_str()).await {
                                    println!("error on sending response; error = {:?}", e);
                                }
                            }
                            Err(e) => {
                                println!("error on decoding from socket; error = {:?}", e);
                            }
                        }
                    }

                    // The connection will be closed at this point as `lines.next()` has returned `None`.
                });
            }
            Err(e) => println!("error accepting socket; error = {:?}", e),
        }
    }
}

fn handle_request(line: &str, db: &Arc<Database>) -> Response {
    let request = match Request::parse(line) {
        Ok(req) => req,
        Err(e) => return Response::Error { msg: e },
    };

    let mut db = db.map.lock().unwrap();
    match request {
        Request::New { class, password } => {
            let did = util::get_did(class);
            // create the new user on chain
            if contract::create_new_account(&did, password) {
                Response::Double {
                    one: did.to_owned(),
                    two: password.to_owned(),
                }
            } else {
                Response::Error {
                    msg: "could not complete creation of account".into(),
                }
            }
        }
        Request::Get { key } => match db.get(&key) {
            Some(value) => Response::Double {
                one: key,
                two: value.clone(),
            },
            None => Response::Error {
                msg: format!("no key {}", key),
            },
        },
        Request::Set { key, value } => {
            let previous = db.insert(key.clone(), value.clone());
            Response::Triple {
                one: key,
                two: value,
                three: previous,
            }
        }
    }
}

impl<'a> Request<'a> {
    fn parse(input: &'a str) -> Result<Request<'a>, String> {
        let mut parts = input.splitn(3, ' ');
        match parts.next() {
            Some("GET") => {
                let key = parts.next().ok_or("GET must be followed by a key")?;
                if parts.next().is_some() {
                    return Err("GET's key must not be followed by anything".into());
                }
                Ok(Request::Get {
                    key: key.to_string(),
                })
            }
            Some("SET") => {
                let key = match parts.next() {
                    Some(key) => key,
                    None => return Err("SET must be followed by a key".into()),
                };
                let value = match parts.next() {
                    Some(value) => value,
                    None => return Err("SET needs a value".into()),
                };
                Ok(Request::Set {
                    key: key.to_string(),
                    value: value.to_string(),
                })
            }
            Some("NEW") => {
                let class = parts.next().ok_or("NEW must be followed by a type")?;
                if class != "sam" && class != "app" {
                    return Err("invalid type of user specified".into());
                }
                let password = parts
                    .next()
                    .ok_or("After specifying the type, you must specify a password")?;

                // check password length and content
                if password.chars().all(char::is_alphabetic)
                    || password.chars().all(char::is_numeric)
                    || password.len() < 8
                {
                    return Err("password must be aplhanumeric and more than 8 characters".into());
                }
                Ok(Request::New { class, password })
            }
            Some(cmd) => Err(format!("unknown command: {}", cmd)),
            None => Err("empty input".into()),
        }
    }
}

impl Response {
    fn serialize(&self) -> String {
        match *self {
            Response::Single(ref one) => format!("{}", one),
            Response::Double { ref one, ref two } => format!("[{}, {}]", one, two),
            Response::Triple {
                ref one,
                ref two,
                ref three,
            } => format!(
                "[{}, {}, {}]",
                one,
                two,
                three.to_owned().unwrap_or_default()
            ),
            Response::Error { ref msg } => format!("error: {}", msg),
        }
    }
}

mod contract {
    use super::util;
    use std::process::Command;

    pub fn create_new_account(did: &str, password: &str) -> bool {
        let password = util::blake2_hash(password);
        let binding = String::from_utf8(password).unwrap_or_default();
        let pw_str = binding.as_str();

        let output = Command::new("cargo")
            .args([
                "contract",
                "call",
                "--contract",
                "5ErTHUWGoxPps2CSZmTEhtpErM7SkKnf5mzeG5cb3UDCe4zQ",
                "--message",
                "create_new_account",
                "--suri",
                "//Alice",
                "--args",
                did,
                pw_str,
                "emptyDidDocument", /* DID Document is not handled yet */
            ]) 
            .current_dir("../sam_os")
            .output()
            .expect("failed to execute process");

        println!("{:?}", output);
        true
    }
}

mod util {
    use blake2::{/* Blake2b512, */ Blake2s256, Digest};
    use rand::{distributions::Alphanumeric, thread_rng, Rng};

    /// generate a did for the user
    pub fn get_did(class: &str) -> String {
        let mut _did = String::with_capacity(60);
        let random_str = gen_random_str(43);
        if class == "sam" {
            _did = format!("did:sam:root:DS{random_str}");
        } else {
            _did = format!("did:sam:apps:DS{random_str}");
        }
        _did
    }

    /// generate random number of alphanumerics
    pub fn gen_random_str(n: u32) -> String {
        let r = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(n as usize)
            .map(|mut e| {
                let a = 32;
                if e.is_ascii_alphabetic() {
                    e = e & !a;
                }
                e
            })
            .collect::<Vec<_>>();
        String::from_utf8_lossy(&r).into()
    }

    /// generate a blake2 hash of input
    pub fn blake2_hash(input: &str) -> Vec<u8> {
        // create a Blake2b512 object
        let mut hasher = Blake2s256::new();

        // write input message
        hasher.update(input);

        // read hash digest and consume hasher
        let res = hasher.finalize();
        res[..].to_owned()
    }
}
