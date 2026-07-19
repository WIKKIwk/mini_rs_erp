struct UnavailableReturnedPaintStore;

#[async_trait]
impl ReturnedPaintStorePort for UnavailableReturnedPaintStore {
    async fn create(
        &self,
        _request: ReturnedPaintRequest,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        Err(ReturnedPaintError::StoreFailed)
    }

    async fn list(
        &self,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<ReturnedPaintRequest>, ReturnedPaintError> {
        Err(ReturnedPaintError::StoreFailed)
    }

    async fn complete(
        &self,
        _request_id: &str,
        _items: Vec<ReturnedPaintItem>,
        _calculation: ReturnedPaintCalculation,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        Err(ReturnedPaintError::StoreFailed)
    }

    async fn save_image(
        &self,
        _image: ReturnedPaintStoredImage,
    ) -> Result<ReturnedPaintStoredImage, ReturnedPaintError> {
        Err(ReturnedPaintError::StoreFailed)
    }

    async fn image(
        &self,
        _image_id: &str,
    ) -> Result<Option<ReturnedPaintStoredImage>, ReturnedPaintError> {
        Err(ReturnedPaintError::StoreFailed)
    }

    async fn delete_image(
        &self,
        _image_id: &str,
        _owner_ref: &str,
    ) -> Result<bool, ReturnedPaintError> {
        Err(ReturnedPaintError::StoreFailed)
    }
}

#[derive(Default)]
pub struct MemoryReturnedPaintStore {
    requests: RwLock<Vec<ReturnedPaintRequest>>,
    images: RwLock<BTreeMap<String, ReturnedPaintStoredImage>>,
}

impl MemoryReturnedPaintStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ReturnedPaintStorePort for MemoryReturnedPaintStore {
    async fn create(
        &self,
        request: ReturnedPaintRequest,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        let mut requests = self.requests.write().await;
        if let Some(existing) = requests.iter().find(|existing| existing.id == request.id) {
            return Ok(existing.clone());
        }
        requests.push(request.clone());
        Ok(request)
    }

    async fn list(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ReturnedPaintRequest>, ReturnedPaintError> {
        let mut requests = self.requests.read().await.clone();
        requests.sort_by(|left, right| {
            right
                .created_at_unix
                .cmp(&left.created_at_unix)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(requests.into_iter().skip(offset).take(limit).collect())
    }

    async fn complete(
        &self,
        request_id: &str,
        items: Vec<ReturnedPaintItem>,
        calculation: ReturnedPaintCalculation,
    ) -> Result<ReturnedPaintRequest, ReturnedPaintError> {
        let mut requests = self.requests.write().await;
        let request = requests
            .iter_mut()
            .find(|request| request.id == request_id)
            .ok_or(ReturnedPaintError::RequestNotFound)?;
        if request.status == ReturnedPaintStatus::Completed {
            return Ok(request.clone());
        }
        request.items = items;
        request.calculation = Some(calculation);
        request.status = ReturnedPaintStatus::Completed;
        request.message = completion_report_message(request);
        Ok(request.clone())
    }

    async fn save_image(
        &self,
        image: ReturnedPaintStoredImage,
    ) -> Result<ReturnedPaintStoredImage, ReturnedPaintError> {
        self.images
            .write()
            .await
            .insert(image.image.image_id.clone(), image.clone());
        Ok(image)
    }

    async fn image(
        &self,
        image_id: &str,
    ) -> Result<Option<ReturnedPaintStoredImage>, ReturnedPaintError> {
        Ok(self.images.read().await.get(image_id).cloned())
    }

    async fn delete_image(
        &self,
        image_id: &str,
        owner_ref: &str,
    ) -> Result<bool, ReturnedPaintError> {
        if self.requests.read().await.iter().any(|request| {
            request
                .image
                .as_ref()
                .is_some_and(|image| image.image_id == image_id)
        }) {
            return Ok(false);
        }
        let mut images = self.images.write().await;
        if images
            .get(image_id)
            .map_or(true, |image| image.owner_ref != owner_ref)
        {
            return Ok(false);
        }
        Ok(images.remove(image_id).is_some())
    }
}

