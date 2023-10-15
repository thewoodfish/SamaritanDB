// Copyright (c) 2023 Algorealm, Inc.

use std::sync::Arc;

use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use async_std::sync::RwLock;
use futures::channel::mpsc::Sender;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{network::ChannelMsg, prelude::*, util};

/// struct to hold data we'll pass to all our request handlers
struct ServerInternalData {
    cfg: Arc<DBConfig>,
    db_state: Arc<RwLock<DBState>>,
    sender: Sender<ChannelMsg>,
}

/// struct to hold incoming data from application clients
#[derive(Debug, Serialize, Deserialize)]
struct ServerIncomingData {
    did: String,
    password: String,
    command: String,
    data: Vec<String>,
    sam_did: String,
}

/// enum representing possible server command
#[derive(Debug, Serialize, Deserialize)]
enum DBServerCommand {
    Insert {
        did: String,
        keys: Vec<String>,
        values: Vec<String>,
        sam_did: Option<String>,
    },
    Get {
        did: String,
        keys: Vec<String>,
        sam_did: Option<String>,
    },
    Del {
        did: String,
        keys: Vec<String>,
        sam_did: Option<String>,
    },
    Invalid,
}

/// handle all INSERT requests
async fn handle_client_req(
    data: web::Data<ServerInternalData>,
    input: web::Json<ServerIncomingData>,
) -> HttpResponse {
    // Here is our Http Response in case of errors
    // before handling any data, check if the application is initialized on this database node
    let parsed_result =
        if let Some(password) = data.db_state.read().await.authenticated.get(&input.did) {
            // compare the password
            if *password == input.password {
                // now handle the command respectively
                match input.command.as_str() {
                    "insert" => {
                        // lets get the keys and values in separate vectors
                        let mut keys = Vec::new();
                        let mut values = Vec::new();
                        let mut is_key = true;

                        for item in input.data.clone() {
                            if item == "::" {
                                is_key = false;
                                continue;
                            }

                            if is_key {
                                keys.push(item);
                            } else {
                                values.push(item);
                            }
                        }

                        Ok(DBServerCommand::Insert {
                            did: input.did.clone(),
                            keys,
                            values,
                            sam_did: if input.sam_did.is_empty() {
                                None
                            } else {
                                Some(input.sam_did.clone())
                            },
                        })
                    }

                    "get" => Ok(DBServerCommand::Get {
                        did: input.did.clone(),
                        keys: input.data.clone(),
                        sam_did: if input.sam_did.is_empty() {
                            None
                        } else {
                            Some(input.sam_did.clone())
                        },
                    }),

                    "del" => Ok(DBServerCommand::Del {
                        did: input.did.clone(),
                        keys: input.data.clone(),
                        sam_did: if input.sam_did.is_empty() {
                            None
                        } else {
                            Some(input.sam_did.clone())
                        },
                    }),
                    _ => Ok(DBServerCommand::Invalid),
                }
            } else {
                // incorrect password
                Err(DBError::AuthenticationError)
            }
        } else {
            // application not intitialized
            Err(DBError::AuthenticationError)
        };

    // The main distinction between the server command and the db command is the fact that the client
    // can request multiple operations to be done at once
    match parsed_result {
        Ok(server_command) => {
            // The main distinction between the server command and the db command is the fact that the client
            // can request multiple operations to be done at once
            if !matches!(server_command, DBServerCommand::Invalid) {
                match server_command {
                    DBServerCommand::Insert {
                        did,
                        keys,
                        values,
                        sam_did,
                    } => {
                        let mut failure_count = 0;
                        let mut cmd_error = serde_json::Value::Null;
                        for (key, value) in keys.iter().zip(values.iter()) {
                            let cmd_sender = data.sender.clone();
                            let db_state = data.db_state.clone();
                            let cfg = data.cfg.clone();

                            // construct command
                            let cmd = Command::Set {
                                did: &did,
                                sam_did: sam_did.as_deref(),
                                key,
                                value,
                            };

                            // TODO; This should be fixed, it should execute immediately without any extra padding
                            let db_result = cmd.parse().execute(cfg, cmd_sender, db_state).await;

                            // we don't return anything for the null values, those are meant for only terminal interactions
                            if db_result != serde_json::Value::Null {
                                if values.len() == 1 {
                                    // we can return to the client immediately after the first op
                                    util::log_info(
                                        "Database server operation complete -> insert()",
                                    );
                                    // we have to check if its an error or not
                                    if db_result["status"] == 200 {
                                        return HttpResponse::Ok().json(db_result.to_string());
                                    } else {
                                        return HttpResponse::InternalServerError()
                                            .json(db_result.to_string());
                                    }
                                } else {
                                    // check for errors
                                    if db_result["status"] != 200 {
                                        failure_count += 1;

                                        // save one error
                                        cmd_error = db_result;
                                    }
                                }
                            }
                            util::log_info("Database server operation complete -> insert()");
                        }

                        // now send response to client
                        if values.len() > 1 {
                            // check for any error
                            if failure_count != 0 {
                                // send error message
                                HttpResponse::InternalServerError().json(cmd_error.to_string())
                            } else {
                                // success
                                HttpResponse::Ok()
                                    .json(util::bake_response(DBServerResponse::Empty).to_string())
                            }
                        } else {
                            // can't reach here
                            HttpResponse::Ok()
                                .json(util::bake_response(DBServerResponse::Empty).to_string())
                        }
                    }
                    DBServerCommand::Get { did, keys, sam_did } => {
                        let mut failure_count = 0;
                        let mut cmd_error = serde_json::Value::Null;
                        let mut get_values = Vec::new();
                        for key in keys.iter() {
                            let cmd_sender = data.sender.clone();
                            let db_state = data.db_state.clone();
                            let cfg = data.cfg.clone();

                            // construct command
                            let cmd = Command::Get {
                                did: &did,
                                sam_did: sam_did.as_deref(),
                                key,
                            };

                            // TODO; This should be fixed, it should execute immediately ( NO need to parse anything again)
                            let db_result = cmd.parse().execute(cfg, cmd_sender, db_state).await;

                            // we don't return anything for the null values, those are meant for only terminal interactions
                            if db_result != serde_json::Value::Null {
                                if keys.len() == 1 {
                                    // we can return to the client immediately after the first op
                                    util::log_info("Database server operation complete -> get()");
                                    // we have to check if its an error or not
                                    if db_result["status"] == 200 {
                                        return HttpResponse::Ok().json(db_result.to_string());
                                    } else {
                                        return HttpResponse::InternalServerError()
                                            .json(db_result.to_string());
                                    }
                                } else {
                                    // check for errors
                                    if db_result["status"] != 200 && db_result["status"] != 127 {
                                        // `not found` is not an error
                                        failure_count += 1;

                                        // save one error
                                        cmd_error = db_result;
                                    } else {
                                        get_values.push(db_result["data"].to_owned());
                                    }
                                }
                            }
                            util::log_info("Database server operation complete -> get()");
                        }

                        // now send response to client
                        if keys.len() > 1 {
                            // check for any error
                            if failure_count != 0 {
                                // send error message
                                HttpResponse::InternalServerError().json(cmd_error.to_string())
                            } else {
                                // success
                                // get all the values requested
                                let mut values = Vec::new();
                                for data in get_values {
                                    values.push(data["value"].to_owned());
                                }

                                HttpResponse::Ok().json(
                                    json!({
                                                "status": 200,
                                                "data": {
                                                    "keys": keys,
                                                    "values": values,
                                                    "length": 0   // indicate we're more than one
                                                }
                                    })
                                    .to_string(),
                                )
                            }
                        } else {
                            // can't reach here
                            HttpResponse::Ok()
                                .json(util::bake_response(DBServerResponse::Empty).to_string())
                        }
                    }
                    DBServerCommand::Del { did, keys, sam_did } => {
                        let mut failure_count = 0;
                        let mut cmd_error = serde_json::Value::Null;
                        for key in keys.iter() {
                            let cmd_sender = data.sender.clone();
                            let db_state = data.db_state.clone();
                            let cfg = data.cfg.clone();

                            // construct command
                            let cmd = Command::Del {
                                did: &did,
                                sam_did: sam_did.as_deref(),
                                key,
                            };

                            // TODO; This should be fixed, it should execute immediately
                            let db_result = cmd.parse().execute(cfg, cmd_sender, db_state).await;

                            // we don't return anything for the null values, those are meant for only terminal interactions
                            if db_result != serde_json::Value::Null {
                                if keys.len() == 1 {
                                    // we can return to the client immediately after the first op
                                    util::log_info("Database server operation complete -> del()");
                                    // we have to check if its an error or not
                                    if db_result["status"] == 200 {
                                        return HttpResponse::Ok().json(db_result.to_string());
                                    } else {
                                        return HttpResponse::InternalServerError()
                                            .json(db_result.to_string());
                                    }
                                } else {
                                    // check for errors
                                    if db_result["status"] != 200 {
                                        failure_count += 1;

                                        // save one error
                                        cmd_error = db_result;
                                    }
                                }
                            }
                            util::log_info("Database server operation complete -> del()");
                        }

                        // now send response to client
                        if keys.len() > 1 {
                            // check for any error
                            if failure_count != 0 {
                                // send error message
                                HttpResponse::InternalServerError().json(cmd_error.to_string())
                            } else {
                                // success
                                HttpResponse::Ok()
                                    .json(util::bake_response(DBServerResponse::Empty).to_string())
                            }
                        } else {
                            // can't reach here
                            HttpResponse::Ok()
                                .json(util::bake_response(DBServerResponse::Empty).to_string())
                        }
                    }
                    DBServerCommand::Invalid => {
                        util::log_error(&format!("invalid command syntax recieved from client"));
                        HttpResponse::InternalServerError()
                            .json(util::bake_error(DBError::CommandParseError))
                    }
                }
            } else {
                HttpResponse::InternalServerError()
                    .json(util::bake_error(DBError::CommandParseError))
            }
        }
        Err(e) => return HttpResponse::InternalServerError().json(util::bake_error(e)),
    }
}

/// set up the HTTP database server to listen and handle incoming requests
#[actix_web::main]
pub async fn setup_server(
    cfg: Arc<DBConfig>,
    db_state: Arc<RwLock<DBState>>,
    sender: Sender<ChannelMsg>,
) -> std::io::Result<()> {
    // lets open the server address for connections
    // let ip = local_ip::get()
    //     .expect_variant("Failed to start database server. Could not retrieve the machine's IP.");

    // open the server connection on all interfaces
    let ip = "0.0.0.0";

    util::log_info(&format!(
        "Database server listening on {}:{}",
        ip,
        cfg.get_server_port()
    ));

    let server_data = web::Data::new(ServerInternalData {
        cfg: cfg.clone(),
        db_state,
        sender,
    });

    // block on it
    async_std::task::block_on(async {
        HttpServer::new(move || {
            App::new()
                // enable logger
                .wrap(middleware::Logger::default())
                .app_data(server_data.clone())
                .app_data(web::JsonConfig::default().limit(4096))
                .service(web::resource("/db/data").route(web::post().to(handle_client_req)))
        })
        .bind((format!("{ip}"), cfg.get_server_port()))?
        .run()
        .await
    })
}
