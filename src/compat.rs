use crate::{api, error::{ApiError, ApiResult}, models::SendRequest, AppState};
use axum::{body::Bytes, extract::{Path, Query, State}, http::HeaderMap, Json};
use serde_json::{json,Value};
use std::collections::HashMap;
use uuid::Uuid;

type Params=HashMap<String,String>;

fn parsed(headers:&HeaderMap,raw:&[u8])->ApiResult<Value>{
    if raw.is_empty(){return Ok(json!({}));}
    let content=headers.get("content-type").and_then(|v|v.to_str().ok()).unwrap_or("");
    if content.starts_with("application/json") {
        serde_json::from_slice(raw).map_err(|_|ApiError::bad("invalid JSON request"))
    } else {
        let form:Params=serde_urlencoded::from_bytes(raw).map_err(|_|ApiError::bad("invalid form request"))?;
        Ok(json!(form))
    }
}
fn get<'a>(data:&'a Value,field:&str)->Option<&'a str>{
    data.get(field).and_then(Value::as_str).filter(|x|!x.is_empty())
}
fn send_request(data:&Value,query:&Params,title_names:&[&str],body_names:&[&str])->ApiResult<SendRequest>{
    fn find(data:&Value,query:&Params,keys:&[&str])->String{
        keys.iter().find_map(|k|get(data,k).or_else(||query.get(*k).map(String::as_str))).unwrap_or("").to_owned()
    }
    let title=find(data,query,title_names);
    let body=find(data,query,body_names);
    let attachments=data.get("attachments").map(|v|serde_json::from_value(v.clone()))
        .transpose().map_err(|_|ApiError::bad("attachments must be an array of Base64 file objects"))?
        .unwrap_or_default();
    let provider_ids:Option<Vec<Uuid>>=data.get("provider_ids").map(|v|serde_json::from_value(v.clone()))
        .transpose().map_err(|_|ApiError::bad("provider_ids must be an array of UUID strings"))?;
    Ok(SendRequest{title,body,attachments,provider_ids})
}
pub async fn serverchan_get(State(state):State<AppState>,Path(sendkey):Path<String>,Query(query):Query<Params>)->ApiResult<Json<Value>>{
    serverchan_run(&state,&sendkey,&json!({}),&query).await
}
pub async fn serverchan_post(State(state):State<AppState>,Path(sendkey):Path<String>,
    headers:HeaderMap,Query(query):Query<Params>,raw:Bytes)->ApiResult<Json<Value>>{
    serverchan_run(&state,&sendkey,&parsed(&headers,&raw)?,&query).await
}
async fn serverchan_run(state:&AppState,path:&str,data:&Value,query:&Params)->ApiResult<Json<Value>>{
    let key=path.strip_suffix(".send").ok_or_else(ApiError::not_found)?;
    let request=send_request(data,query,&["title","text"],&["desp","content","body"])?;
    let id=api::enqueue_with_key(state,key,request).await?;
    Ok(Json(json!({"code":0,"message":"success","data":{"pushid":id.to_string(),"queued":true}})))
}
pub async fn pushplus_get(State(state):State<AppState>,Query(query):Query<Params>)->ApiResult<Json<Value>>{
    pushplus_run(&state,&json!({}),&query).await
}
pub async fn pushplus_post(State(state):State<AppState>,headers:HeaderMap,Query(query):Query<Params>,raw:Bytes)->ApiResult<Json<Value>>{
    pushplus_run(&state,&parsed(&headers,&raw)?,&query).await
}
async fn pushplus_run(state:&AppState,data:&Value,query:&Params)->ApiResult<Json<Value>>{
    let key=get(data,"token").or_else(||query.get("token").map(String::as_str)).ok_or_else(ApiError::unauthorized)?;
    let req=send_request(data,query,&["title"],&["content","message"])?;
    let id=api::enqueue_with_key(state,key,req).await?;
    Ok(Json(json!({"code":200,"msg":"Message queued","data":{"id":id.to_string()}})))
}
pub async fn gotify(State(state):State<AppState>,headers:HeaderMap,Query(query):Query<Params>,raw:Bytes)->ApiResult<Json<Value>>{
    let data=parsed(&headers,&raw)?;
    let key=get(&data,"token").or_else(||query.get("token").map(String::as_str))
        .ok_or_else(ApiError::unauthorized)?;
    let req=send_request(&data,&query,&["title"],&["message"])?;
    let id=api::enqueue_with_key(&state,key,req).await?;
    Ok(Json(json!({"id":id,"title":get(&data,"title").unwrap_or(""),"message":get(&data,"message").unwrap_or(""),"priority":5})))
}
pub async fn ntfy(State(state):State<AppState>,headers:HeaderMap,Path(key):Path<String>,raw:Bytes)->ApiResult<Json<Value>>{
    let body=String::from_utf8(raw.to_vec()).map_err(|_|ApiError::bad("ntfy body must be UTF-8"))?;
    let title=headers.get("title").and_then(|v|v.to_str().ok()).unwrap_or("Notification").to_string();
    let id=api::enqueue_with_key(&state,&key,SendRequest{title,body,provider_ids:None,attachments:vec![]}).await?;
    Ok(Json(json!({"id":id,"event":"message","queued":true})))
}
pub async fn bark(State(state):State<AppState>,Path((key,title,body)):Path<(String,String,String)>)->ApiResult<Json<Value>>{
    let id=api::enqueue_with_key(&state,&key,SendRequest{title,body,provider_ids:None,attachments:vec![]}).await?;
    Ok(Json(json!({"code":200,"message":"success","data":{"id":id.to_string()}})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serverchan_parameter_aliases() {
        let params=Params::new();
        let req=send_request(&json!({"text":"标题","desp":"正文"}),&params,&["title","text"],&["desp"]).unwrap();
        assert_eq!(req.title,"标题");
        assert_eq!(req.body,"正文");
    }
    #[test]
    fn form_parsing(){
        let headers=HeaderMap::new();
        let p=parsed(&headers,b"title=Hello+world&desp=some%20text").unwrap();
        assert_eq!(get(&p,"title"),Some("Hello world"));
    }
}
