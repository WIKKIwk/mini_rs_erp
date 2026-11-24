#[derive(Debug, Deserialize)]
pub struct RefQuery {
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RefItemQuery {
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    pub item_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AdminProfileAvatarQuery {
    pub role: Option<String>,
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    pub token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ItemQuery {
    pub q: Option<String>,
    pub warehouse: Option<String>,
    pub item_code: Option<String>,
    pub order_id: Option<String>,
    pub parent: Option<String>,
    pub group: Option<String>,
    pub limit: Option<String>,
    pub offset: Option<String>,
}
