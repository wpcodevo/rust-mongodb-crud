use crate::response::{NoteData, NoteListResponse, NoteResponse, SingleNoteResponse};
use crate::{
    error::Error::*, model::NoteModel, schema::CreateNoteSchema, schema::UpdateNoteSchema, Result,
};
use chrono::prelude::*;
use futures::StreamExt;
use mongodb::bson::{doc, oid::ObjectId, Document};
use mongodb::options::{FindOneAndUpdateOptions, FindOptions, IndexOptions, ReturnDocument};
use mongodb::{bson, options::ClientOptions, Client, Collection, IndexModel};
use std::str::FromStr;

#[derive(Clone, Debug)]
pub struct DB {
    pub note_collection: Collection<NoteModel>,
    pub collection: Collection<Document>,
}

impl DB {
    pub async fn init() -> Result<Self> {
        let mongodb_uri: String = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set.");
        let database_name: String =
            std::env::var("MONGO_INITDB_DATABASE").expect("MONGO_INITDB_DATABASE must be set.");
        let mongodb_note_collection: String =
            std::env::var("MONGODB_NOTE_COLLECTION").expect("MONGODB_NOTE_COLLECTION must be set.");

        let mut client_options = ClientOptions::parse(mongodb_uri).await?;
        client_options.app_name = Some(database_name.to_string());

        let client = Client::with_options(client_options)?;
        let database = client.database(database_name.as_str());

        let note_collection = database.collection(mongodb_note_collection.as_str());
        let collection = database.collection::<Document>(mongodb_note_collection.as_str());

        println!("✅ Database connected successfully");

        Ok(Self {
            note_collection,
            collection,
        })
    }

    pub async fn fetch_notes(&self, limit: i64, page: i64) -> Result<NoteListResponse> {
        let find_options = FindOptions::builder()
            .limit(limit)
            .skip(u64::try_from((page - 1) * limit).unwrap())
            .build();

        let mut cursor = self
            .note_collection
            .find(None, find_options)
            .await
            .map_err(MongoQueryError)?;

        let mut json_result: Vec<NoteResponse> = Vec::new();
        while let Some(doc) = cursor.next().await {
            json_result.push(self.doc_to_note(&doc.unwrap())?);
        }

        let json_note_list = NoteListResponse {
            status: "success".to_string(),
            results: json_result.len(),
            notes: json_result,
        };

        Ok(json_note_list)
    }

    pub async fn create_note(&self, body: &CreateNoteSchema) -> Result<Option<SingleNoteResponse>> {
        let published = body.published.to_owned().unwrap_or(false);
        let category = body.category.to_owned().unwrap_or("".to_string());
        let serialized_data = bson::to_bson(&body).map_err(MongoSerializeBsonError)?;
        let document = serialized_data.as_document().unwrap();
        let options = IndexOptions::builder().unique(true).build();
        let index = IndexModel::builder()
            .keys(doc! {"title": 1})
            .options(options)
            .build();

        self.note_collection
            .create_index(index, None)
            .await
            .expect("error creating index!");

        let datetime = Utc::now();

        let mut doc_with_dates = doc! {"createdAt": datetime, "updatedAt": datetime, "published": published, "category": category};
        doc_with_dates.extend(document.clone());

        let insert_result = self
            .collection
            .insert_one(&doc_with_dates, None)
            .await
            .map_err(|e| {
                if e.to_string()
                    .contains("E11000 duplicate key error collection")
                {
                    return MongoDuplicateError(e);
                }
                return MongoQueryError(e);
            })?;

        let new_id = insert_result
            .inserted_id
            .as_object_id()
            .expect("issue with new _id");

        let note_doc = self
            .note_collection
            .find_one(doc! {"_id":new_id }, None)
            .await
            .map_err(MongoQueryError)?;

        if note_doc.is_none() {
            return Ok(None);
        }

        let note_response = SingleNoteResponse {
            status: "success".to_string(),
            data: NoteData {
                note: self.doc_to_note(&note_doc.unwrap()).unwrap(),
            },
        };

        Ok(Some(note_response))
    }

    pub async fn get_note(&self, id: &str) -> Result<Option<SingleNoteResponse>> {
        let oid = ObjectId::from_str(id).map_err(|_| InvalidIDError(id.to_owned()))?;

        let note_doc = self
            .note_collection
            .find_one(doc! {"_id":oid }, None)
            .await
            .map_err(MongoQueryError)?;

        if note_doc.is_none() {
            return Ok(None);
        }

        let note_response = SingleNoteResponse {
            status: "success".to_string(),
            data: NoteData {
                note: self.doc_to_note(&note_doc.unwrap()).unwrap(),
            },
        };

        Ok(Some(note_response))
    }

    pub async fn edit_note(
        &self,
        id: &str,
        body: &UpdateNoteSchema,
    ) -> Result<Option<SingleNoteResponse>> {
        let oid = ObjectId::from_str(id).map_err(|_| InvalidIDError(id.to_owned()))?;
        let query = doc! {
            "_id": oid,
        };

        let find_one_and_update_options = FindOneAndUpdateOptions::builder()
            .return_document(ReturnDocument::After)
            .build();

        let serialized_data = bson::to_bson(body).map_err(MongoSerializeBsonError)?;
        let document = serialized_data.as_document().unwrap();
        let update = doc! {"$set": document};

        let note_doc = self
            .note_collection
            .find_one_and_update(query, update, find_one_and_update_options)
            .await
            .map_err(MongoQueryError)?;

        if note_doc.is_none() {
            return Ok(None);
        }

        let note_response = SingleNoteResponse {
            status: "success".to_string(),
            data: NoteData {
                note: self.doc_to_note(&note_doc.unwrap()).unwrap(),
            },
        };

        Ok(Some(note_response))
    }

    pub async fn delete_note(&self, id: &str) -> Result<Option<()>> {
        let oid = ObjectId::from_str(id).map_err(|_| InvalidIDError(id.to_owned()))?;

        let result = self
            .collection
            .delete_one(doc! {"_id":oid }, None)
            .await
            .map_err(MongoQueryError)?;

        if result.deleted_count == 0 {
            return Ok(None);
        }

        Ok(Some(()))
    }

    fn doc_to_note(&self, note: &NoteModel) -> Result<NoteResponse> {
        let note_response = NoteResponse {
            id: note.id.to_hex(),
            title: note.title.to_owned(),
            content: note.content.to_owned(),
            category: note.category.to_owned().unwrap(),
            published: note.published.unwrap(),
            createdAt: note.createdAt,
            updatedAt: note.updatedAt,
        };

        Ok(note_response)
    }
}
