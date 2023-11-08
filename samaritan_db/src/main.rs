#![warn(rust_2018_idioms)]

mod sam_prelude;
mod util;
use contract::interface;
use sam_prelude::*;
// use serde::Serialize;
// use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_stream::StreamExt;
use tokio_util::codec::{Framed, LinesCodec};

use futures::SinkExt;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use std::{env, thread};
use util::*;

mod contract; 
mod ipfs;

#[tokio::main] 
async fn main() -> Result<(), Box<dyn Error>> {
    // Parse the address we're going to run this server on
    // and set up our TCP listener to accept connections.
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8888".to_string());

    let listener = TcpListener::bind(&addr).await?;
    println!("Listening on: {}", addr);

    // Create the shared state of this server that will be shared amongst all
    // clients. Note the usage of `Arc` here which will be used to ensure that
    // each independently spawned client will have a reference to the in-memory
    // database and config.
    let rdb = Arc::new(Database::new());
    let rconfig = Arc::new(Config::new());

    // clone the database and config
    let db = rdb.clone();
    let config = rconfig.clone();

    // initialize IPFS
    // if !ipfs::init_ipfs() {
    //     // shut down the database
    //     println!("error: could not start IPFS daemon.");
    //     return Ok(())
    // }

    // This is what controls whether the database is in full mode
    // We're going to use conditional compilation here

    // Spawn a new client that synchronizes with IPFS.
    tokio::spawn(async move {
        let mut count = 1;
        loop {
            // sleep for some seconds
            thread::sleep(Duration::from_secs(config.ipfs_sync_interval));
            println!("[{}] ->  sync cycle", count);
            count += 1;

            // sync
            db.sync_files(&config);
        }
    });

    loop {
        match listener.accept().await {
            Ok((socket, _)) => {
                // After getting a new connection first we see a clone of the database & config
                // being created, which is creating a new reference for this connected
                // client to use.
                let db = rdb.clone();
                let config = rconfig.clone();

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
                                let response = handle_request(&line, &db, &config);
                                let response = response.serialize();

                                // log state of database
                                db.snapshot();

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

fn handle_request(line: &str, db: &Arc<Database>, config: &Arc<Config>) -> Response {
    // make sure that the application is a real one EXCEPT during new
    // let request: Request<'_>;
    // if !line.starts_with("new::") || !line.starts_with("NEW::") {
    //     // check for auth data
    //     let mut chunks = line.split("~~");
    //     // the first chunk contains the auth data
    //     let mut first_chunk = chunks.next().unwrap_or_default().split("::");
    //     let did = first_chunk.next().unwrap_or_default();
    //     let keys = first_chunk.next().unwrap_or_default();

    //     if did.len() > 8 && keys.len() > 10 {
    //         if !db.account_is_alive(did, keys) {
    //             // check the smart contract for account entry
    //             let (exists, _) = interface::account_is_auth(&config, &did, keys);
    //             if !exists {
    //                 return Response::Error {
    //                     msg: format!(
    //                         "account with DID:'{}' and keys:'{}' not recognized",
    //                         did, keys
    //                     ),
    //                 };
    //             }
    //         }
    //     } else {
    //         return Response::Error {
    //             msg: format!(
    //                 "account with DID:'{}' and keys:'{}' not recognized",
    //                 did, keys
    //             ),
    //         };
    //     }

    //     request = match Request::parse(chunks.next().unwrap_or_default()) {
    //         Ok(req) => req,
    //         Err(e) => return Response::Error { msg: e },
    //     };
    // } else {
    //     request = match Request::parse(line) {
    //         Ok(req) => req,
    //         Err(e) => return Response::Error { msg: e },
    //     };
    // }

    let request = match Request::parse(line) {
        Ok(req) => req,
        Err(e) => return Response::Error { msg: e },
    };

    match request {
        Request::New { class, password } => {
            let did = get_did(class);
            // set up hash table for DID
            let (success, cid) = ipfs::set_up_ht();
            if success {
                // create the new user on chain
                if interface::create_new_account(&config, &did, password, &cid) {
                    Response::Double {
                        one: did.to_owned(),
                        two: password.to_owned(),
                    }
                } else {
                    Response::Error {
                        msg: "could not complete account creation".into(),
                    }
                }
            } else {
                Response::Error {
                    msg: "could not complete account creation".into(),
                }
            }
        }
        Request::Auth { did, password } => {
            // check the database cache for an entry
            if !db.account_is_alive(did, password) {
                // check the smart contract for account entry
                let (exists, root_hash_cid) = interface::account_is_auth(&config, &did, password);

                if !exists {
                    // if we just want to check if account exists on chain
                    if password == "null" {
                        if root_hash_cid.len() < 5 {
                            return Response::Error {
                                msg: format!(
                                    "account with DID:'{}' and password:'{}' not recognized",
                                    did, password
                                ),
                            };
                        }
                    } else {
                        return Response::Error {
                            msg: format!(
                                "account with DID:'{}' and password:'{}' not recognized",
                                did, password
                            ),
                        };
                    }
                } else {
                    // add to database auth_list for high speed auth
                    db.add_auth_account(&did, password);

                    // for all the apps that have been initialized
                    // fetch the data the apps hold for the samaritan
                    db.fetch_sam_data(config, &did);
                }
            }

            Response::Single(did.to_owned())
        }
        Request::Init { did, password } => {
            let (_, app_ht_cid) = interface::account_is_auth(&config, &did, password);

            // add to database auth_list for high speed auth
            db.add_auth_account(&did, password);

            // fetch and read hash table into memory
            db.populate_app_ht(did, &app_ht_cid);

            // start to populate the database with IPFS data
            db.populate_db(config, &did);

            Response::Single(did.to_owned())
        }
        Request::Revoke {
            revoker_did,
            app_did,
            revoke,
        } => {
            // check for auth
            if !db.account_is_auth(&config, &revoker_did) {
                return Response::Error {
                    msg: format!("account with did:'{}' not recognized", revoker_did),
                };
            }

            if !db.account_is_auth(&config, &app_did) {
                return Response::Error {
                    msg: format!("account with did:'{}' not recognized", app_did),
                };
            }

            // calculate hashkey
            let hash_key: HashKey = get_hashkey(app_did, revoker_did);
            if db.revoke(&config, hash_key, app_did, revoke) {
                Response::Single(app_did.to_owned())
            } else {
                Response::Error {
                    msg: format!("no links found between '{}' and '{}'", revoker_did, app_did),
                }
            }
        }
        Request::Get {
            subject_did,
            key,
            object_did,
        } => {
            // calculate hashkey
            let hash_key: HashKey = get_hashkey(subject_did, object_did);
            let data = db.get(config, hash_key, key, subject_did, object_did, true);
            let mut revoked = false;
            // first remove the revoked flags
            let mut data = data
                .iter()
                .filter(|s| {
                    if *s != "revoked" {
                        true
                    } else {
                        revoked = true;
                        false
                    }
                })
                .map(|s| s.to_owned())
                .collect::<Vec<String>>();

            // if an app is selecting data about a Samaritan, it first checks the file (app + sam),
            // If not found, we fist check if the app has the proper permission
            if data.len() == 0 && !object_did.is_empty() {
                // check if its not revoked
                if !revoked {
                    // search from Samaritan personal data
                    let hash_key: HashKey = get_hashkey(object_did, object_did);
                    data = db.get(config, hash_key, key, object_did, object_did, true);

                    // remove 'revoked' again, if any
                    data = data
                        .iter()
                        .filter(|s| {
                            if *s != "revoked" {
                                true
                            } else {
                                revoked = true;
                                false
                            }
                        })
                        .map(|s| s.to_owned())
                        .collect::<Vec<String>>();
                }
            }

            let mut collator: Vec<String> = Vec::new();
            // because of the one step recursion of the get() function, we'll split possible concatenations
            let _ = data
                .iter()
                .map(|s| {
                    if s.contains("%%%") {
                        let _ = s
                            .split("%%%")
                            .filter(|s| !(*s).is_empty())
                            .map(|e| collator.push(e.to_string()))
                            .collect::<()>();
                    } else {
                        collator.push(s.clone());
                    }
                })
                .collect::<()>();

            let data = serde_json::to_value(&collator).unwrap_or_default();

            Response::Double {
                one: json_stringify(&key.split(';').collect::<Vec<&str>>()),
                two: json_stringify(&data),
            }
        }
        Request::Del {
            subject_did,
            key,
            object_did,
        } => {
            // calculate hashkey
            let hash_key: HashKey = get_hashkey(subject_did, object_did);
            let data = db.del(config, hash_key, key, subject_did, object_did);
            let mut collator: Vec<String> = Vec::new();
            // because of the one step recursion of the del() function, we'll split possible concatenations
            let _ = data
                .iter()
                .map(|s| {
                    if s.contains("%%%") {
                        let _ = s
                            .split("%%%")
                            .map(|e| collator.push(e.to_string()))
                            .collect::<()>();
                    } else {
                        collator.push(s.clone());
                    }
                })
                .collect::<()>();

            let data = serde_json::to_value(&collator).unwrap_or_default();

            Response::Double {
                one: json_stringify(&key.split(';').collect::<Vec<&str>>()),
                two: json_stringify(&data),
            }
        }
        Request::Insert {
            subject_did,
            key,
            value,
            object_did,
        } => {
            let hash_key: HashKey = get_hashkey(subject_did, object_did);
            let previous = db.insert(config, key, hash_key, value.clone());
            // convert to json value
            let prev = serde_json::to_value(&previous).unwrap_or_default();

            // write file metadata
            db.write_metadata(hash_key, subject_did, object_did);
            Response::Single(serde_json::to_string(&prev).unwrap_or_default())
        }
        Request::GetAll { app_did, sam_did } => {
            // check for auth
            if !db.account_is_auth(&config, &app_did) {
                return Response::Error {
                    msg: format!("account with did:'{}' not recognized", app_did),
                };
            }

            // calculate hashkey
            let hash_key: HashKey = get_hashkey(app_did, sam_did);

            match db.get_all(config, hash_key, app_did, sam_did) {
                Some(value) => Response::Double {
                    one: app_did.to_owned(),
                    two: value.to_owned(),
                },
                None => Response::Error {
                    msg: format!("data not found: '{}'", sam_did),
                },
            }
        }
    }
}