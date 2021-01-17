use crate::model::*;
use crate::error::*;

use actix_web::client::{Client, ClientBuilder};

use serde::{Deserialize, Serialize};


use std::time::Duration;
use chrono::Utc;


use select::document::Document as HTMLDocument;
use select::predicate::{Attr};



use log::{error};





#[allow(non_snake_case)]
#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct CommentsPostForm<'a> {
    id: &'a str,
    no: usize,
    cmt_id: &'a str,
    cmt_no: usize,
    e_s_n_o: Option<&'a str>,
    comment_page: usize,
    sort: &'a str,
    prevCnt: usize,
    _GALLTYPE_: &'a str,
}

async fn back_off<F, O, E>(delay: u64, max_delay: u64, f: impl Fn() -> F) -> Result<O, E>
where F: futures::Future<Output = Result<O, E>>,
{
    let mut i = 0;
    loop {
        let res = f().await;
        if res.is_ok() || i*delay >= max_delay {
            break res;
        }
        println!("{} {}", "backoff..", i);
        i += 1;
        actix::clock::delay_for(Duration::from_millis(delay*i)).await;
    }
}

#[derive(Clone)]
pub struct Crawler {
    pub client: Client,
    host: String,
    e_s_n_o: Option<String>,
    delay: Duration,
}
impl<'a> Crawler {
    pub fn new() -> Self {
        let client = ClientBuilder::new()
            .header(
                "User-Agent",
                "Mozilla/5.0 (X11; Linux x86_64; rv:84.0) Gecko/20100101 Firefox/84.0",
            )
            .finish();
        Crawler {
            client,
            host: String::from("https://gall.dcinside.com"),
            e_s_n_o: None,
            delay: Duration::from_millis(100),
        }
    }
    pub fn delay(mut self, millis: u64) -> Self {
        self.delay = Duration::from_millis(millis);
        self
    }
    pub async fn realtime_hot_galleries(&self) -> Result<Vec<GalleryIndex>, CrawlerError> {
        let jsonp_callback_func = format!(
            "jQuery3210837750950307798_{}",
            Utc::now().timestamp_millis()
        );
        let path = format!(
            "https://json2.dcinside.com/json1/ranking_gallery.php?jsoncallback={}&_={}",
            jsonp_callback_func,
            Utc::now().timestamp_millis()
        );
        Ok(back_off(1000, 1000*10, || async {
            let bytes = self
                .client
                .get(path.as_str())
                .header("Referer", "https://gall.dcinside.com/")
                .send().await?
                .body().limit(1024*1024*8).await?;
            let text = std::str::from_utf8(&bytes)?;
            let trimed = text.trim();
            let jsonp_contents = &trimed[jsonp_callback_func.len() + 1..trimed.len() - 1];
            let mut galleries: Vec<GalleryIndex> = serde_json::from_str(&jsonp_contents)?;
            for g in galleries.iter_mut() {
                g.kind = GalleryKind::Major;
            }
            Ok::<_, CrawlerError>(galleries)
        }).await?)
    }
    pub async fn realtime_hot_minor_galleries(&self) -> Result<Vec<GalleryIndex>, CrawlerError> {
        let jsonp_callback_func = format!(
            "jQuery32107665147071438096_{}",
            Utc::now().timestamp_millis()
        );
        let path = format!(
            "https://json2.dcinside.com/json1/mgallmain/mgallery_ranking.php?jsoncallback={}&_={}",
            jsonp_callback_func,
            Utc::now().timestamp_millis()
        );
        Ok(back_off(1000, 1000*10, || async {
            let bytes = self
                .client
                .get(path.as_str())
                .header("Referer", "https://gall.dcinside.com/m")
                .send().await?
                .body().limit(1024*1024*8).await?;
            let text = std::str::from_utf8(&bytes)?;
            let trimed = text.trim();
            let jsonp_contents = &trimed[jsonp_callback_func.len() + 1..trimed.len() - 1];
            let mut galleries: Vec<GalleryIndex> = serde_json::from_str(&jsonp_contents)?;
            for g in galleries.iter_mut() {
                g.kind = GalleryKind::Minor;
            }
            Ok::<_, CrawlerError>(galleries)
        }).await?)
    }
    pub async fn document_indexes_after(
        &mut self,
        gallery: &GalleryIndex,
        last_document_id: usize,
        start_page: usize,
    ) -> Result<Vec<Result<DocumentIndex, DocumentParseError>>, CrawlerError> {
        let mut docs = Vec::new();
        for i in start_page..1000 {
            let next_docs = self.document_indexes(gallery, i).await?;
            if next_docs.is_empty() {
                break;
            } 
            if !next_docs.iter().any(|d| d.is_ok()) {
                break;
            }
            docs.extend(next_docs);
            if docs.iter().rev().find_map(|d| d.as_ref().ok()).unwrap().id <= last_document_id {
                break;
            }
            actix::clock::delay_for(self.delay).await;
            //actix::clock::delay_for(Duration::from_millis((rand::random::<f32>() * 1000.0) as u64)).await;
        }
        Ok(docs.into_iter().filter(|t| match t { Ok(d) => d.id > last_document_id, Err(_e) => true } ).collect())
    }
    pub async fn comments(
        &mut self,
        gallery: &GalleryIndex,
        doc_id: usize,
    ) -> Result<Vec<Comment>, CrawlerError> {
        let mut comms = Vec::new();
        for i in 1..1000 {
            let (next_comms, max_page) = self._comments(&gallery, doc_id, i, None).await?;
            if next_comms.is_empty() {
                break;
            }
            actix::clock::delay_for(self.delay).await;
            //actix::clock::delay_for(Duration::from_millis((rand::random::<f32>() * 1000.0) as u64)).await;
            comms.extend(next_comms.into_iter().rev());
            if max_page <= i {
                break;
            }
        }
        Ok(comms.into_iter().rev().collect())
    }
    pub async fn documents(
        &mut self,
        gallery: &GalleryIndex,
        page: usize
    ) -> Result<Vec<Result<Document, CrawlerError>>, CrawlerError> {
        let mut documents = Vec::new();
        for res in self.document_indexes(&gallery, page).await? {
            let doc: Result<Document, CrawlerError> = match res {
                Ok(index) => {
                    let id = index.id;
                    let comments = if index.comment_count > 0 {
                        Some(self.comments(&gallery, id).await)
                    } else { 
                        None
                    };
                    // HACKIT: block by dcinside
                    let body: Option<Result<String, CrawlerError>> = None; //Some(self.document_body(&gallery, id).await);
                    match (comments, body) {
                        (Some(Ok(comms)),   Some(Ok(body))  ) => Ok(Document { index: index, comments: Some(comms), body: Some(body) }),
                        (None,              Some(Ok(body))  ) => Ok(Document { index: index, comments: None, body: Some(body) }),
                        (Some(Ok(comms)),   None            ) => Ok(Document { index: index, comments: Some(comms), body: None }),
                        (None,              None            ) => Ok(Document { index: index, comments: None, body: None }),
                        (Some(Err(err)),    _               ) => Err(err.into()),
                        (_,                 Some(Err(err))  ) => Err(err.into()),
                    }
                },
                Err(err) => Err(err.into()),
            };
            documents.push(doc);
        }
        Ok(documents)
    }
    pub async fn document_body(
        &mut self,
        gallery: &GalleryIndex,
        id: usize
    ) -> Result<String, CrawlerError> {
        let path = format!("{}/board/view/?id={}&no={}&page=1", self.host, gallery.id, id);
        let referer = format!("{}/board/lists?id={}", self.host, gallery.id);
        Ok(back_off(1000, 1000*10, || async {
            let bytes = self
                .client
                .get(path.as_str())
                .header("Referer", referer.as_str())
                .send().await?
                .body().limit(1024*1024*8).await?;
            let text = std::str::from_utf8(&bytes)?;
            Ok::<_, CrawlerError>(parse_document_body(text, &gallery.id, id)?)
        }).await?)
    }
    pub async fn documents_after(
        &mut self,
        gallery: &GalleryIndex,
        last_document_id: usize,
        start_page: usize,
    ) -> Result<Vec<Result<Document, CrawlerError>>, CrawlerError> {
        let mut documents = Vec::new();
        for res in self.document_indexes_after(&gallery, last_document_id, start_page).await? {
            let doc: Result<Document, CrawlerError> = match res {
                Ok(index) => {
                    let id = index.id;
                    let comments = if index.comment_count > 0 {
                        Some(self.comments(&gallery, id).await)
                    } else { 
                        None
                    };
                    // HACKIT: block by dcinside
                    let body: Option<Result<String, CrawlerError>> = None; //Some(self.document_body(&gallery, id).await);
                    match (comments, body) {
                        (Some(Ok(comms)),   Some(Ok(body))  ) => Ok(Document { index: index, comments: Some(comms), body: Some(body) }),
                        (None,              Some(Ok(body))  ) => Ok(Document { index: index, comments: None, body: Some(body) }),
                        (Some(Ok(comms)),   None            ) => Ok(Document { index: index, comments: Some(comms), body: None }),
                        (None,              None            ) => Ok(Document { index: index, comments: None, body: None }),
                        (Some(Err(err)),    _               ) => Err(err.into()),
                        (_,                 Some(Err(err))  ) => Err(err.into()),
                    }
                },
                Err(err) => Err(err.into()),
            };
            documents.push(doc);
        }
        Ok(documents)
    }
    async fn _comments(
        &mut self,
        gallery: &GalleryIndex,
        doc_id: usize,
        page: usize,
        last_root_comment_id: Option<usize>,
    ) -> Result<(Vec<Comment>, usize), CrawlerError> {
        let path = format!("{}/board/comment", self.host);
        if self.e_s_n_o.is_none() {
            self.document_indexes(&gallery, 1).await?;
        }
        let form = CommentsPostForm {
            id: &gallery.id,
            no: doc_id,
            cmt_id: &gallery.id,
            cmt_no: doc_id,
            e_s_n_o: self.e_s_n_o.as_deref(),
            comment_page: page,
            sort: if page == 1 { "" } else { "D" },
            prevCnt: 0,
            _GALLTYPE_: "G",
        };
        Ok(back_off(1000, 1000*10, || async {
            let bytes = self
               .client
               .post(path.as_str())
               .header("Accept", "application/json, text/javascript, */*; q=0.01")
               .header("Accept-Encoding", "gzip, deflate, br")
               .header("Content-Type", "application/x-www-form-urlencoded; charset=UTF-8")
               .header("Origin", "https://gall.dcinside.com")
               .header("Host", "gall.dcinside.com")
               .header("Referer", format!("https://gall.dcinside.com/board/view/?id={}&no={}&_rk=tDl&page=1", gallery.id, doc_id))
               .header("X-Requested-With", "XMLHttpRequest")
               .header("Cache-Control", "no-cache")
               .header("Pragma", "no-cache")
               .send_form(&form).await?
               .body().limit(1024*1024*8).await?;
            let text = std::str::from_utf8(&bytes)?;
            Ok::<_, CrawlerError>(parse_comments(text, &gallery.id, doc_id, last_root_comment_id)?)
        }).await?)
    }
    pub async fn document_indexes(
        &mut self,
        gallery: &GalleryIndex,
        page: usize,
    ) -> Result<Vec<Result<DocumentIndex, DocumentParseError>>, CrawlerError> {
        let path = match gallery.kind {
            GalleryKind::Major => format!(
                "{}/board/lists?id={}&list_num=100&page={}",
                self.host, gallery.id, page
            ),
            GalleryKind::Minor => format!(
                "{}/mgallery/board/lists?id={}&list_num=100&page={}",
                self.host, gallery.id, page
            ),
            _ => panic!("What's this?"),
        };
        let (e_s_n_o, res) = back_off(1000, 1000*10, || async {
            let bytes = self
                .client
                .get(path.as_str())
                .header("Referer", "https://gall.dcinside.com/")
                .send().await?
                .body().limit(1024*1024*8).await?;
            let text = std::str::from_utf8(&bytes)?;
            if text.starts_with("<script type=\"text/javascript\">location.replace(\"/error/adault") {
                Err(DocumentParseError::AdultPage)?;
            }
            let e_s_n_o = Some(HTMLDocument::from(text)
                .select(Attr("id", "e_s_n_o"))
                .next()
                .ok_or(DocumentParseError::Select { path: ".e_s_n_o", html: text.to_string() })?
                .attr("value")
                .ok_or(DocumentParseError::Select { path: ".e_s_n_o@value", html: text.to_string() })?.to_string());
            Ok::<_, CrawlerError>((e_s_n_o, parse_document_indexes(text, &gallery.id)?))
        }).await?;
        self.e_s_n_o = e_s_n_o;
        Ok(res)
    }
}
impl Default for Crawler {
    fn default() -> Self {
        Crawler::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[actix_rt::test]
    async fn realtime_hot_galleries() {
        let crawler = Crawler::new();
        let res = crawler.realtime_hot_galleries().await.unwrap();
        assert!(!res.is_empty());
        assert!(!res[0].id.is_empty());
        assert!(!res[0].name.is_empty());
        assert_eq!(res[0].kind, GalleryKind::Major);
    }
    #[actix_rt::test]
    async fn realtime_hot_minor_galleries() {
        let crawler = Crawler::new();
        let res = crawler.realtime_hot_minor_galleries().await.unwrap();
        assert!(!res.is_empty());
        assert!(!res[0].id.is_empty());
        assert!(!res[0].name.is_empty());
        assert_eq!(res[0].kind, GalleryKind::Minor);
    }
    #[actix_rt::test]
    async fn document_indexes() {
        let mut crawler = Crawler::new();
        let gallery = GalleryIndex {
            id: String::from("programming"),
            name: String::from("프로그래밍"),
            kind: GalleryKind::Major,
            rank: None,
        };
        let res = crawler.document_indexes(&gallery, 1).await.unwrap();
        assert!(!res.is_empty());
        assert!(res.len() >= 90);
        assert!(res.iter().any(|d| match d { Ok(d) => d.comment_count > 0, Err(_) => false}));
        assert!(res.iter().any(|d| match d { Ok(d) => if DocumentKind::Picture == d.kind {
            true
        } else {
            false
        }, Err(_) => false }));
    }
    #[actix_rt::test]
    async fn comments() {
        let mut crawler = Crawler::new();
        let gallery = GalleryIndex {
            
            id: String::from("programming"),
            name: String::from("프로그래밍"),
            kind: GalleryKind::Major,
            rank: None,
        };
        let res = crawler.comments(&gallery, 1595404).await.unwrap();
        assert!(!res.is_empty());
        assert!(res.len() >= 1);
        assert!(!res.iter().any(|c| match &c.author {
            User::Static { id, ..  } => id.is_empty(),
            User::Dynamic { ip, .. } => ip.is_empty(),
            _ => false,
        }));
        /*assert!(res.iter().any(|c| d.comment_count > 0));
        assert!(res.iter().any(|c| if DocumentKind::Picture == d.kind {
            true
        } else {
            false
        }));*/
    }
    #[actix_rt::test]
    async fn documents() {
        let mut crawler = Crawler::new();
        let gallery = GalleryIndex {
            id: String::from("lovegame"),
            name: String::from("이승기"),
            kind: GalleryKind::Major,
            rank: None,
        };
        let res = crawler.documents(&gallery, 2).await.unwrap();
        assert!(!res.is_empty());
        assert!(res.len() >= 1);
        for d in res {
            assert!(d.is_ok());
        }
        /*assert!(res.iter().any(|c| d.comment_count > 0));
        assert!(res.iter().any(|c| if DocumentKind::Picture == d.kind {
            true
        } else {
            false
        }));*/
    }
}
