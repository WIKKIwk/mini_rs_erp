impl QolipService {

    pub async fn upsert_location(
        &self,
        mut input: QolipLocationUpsert,
        principal: &Principal,
    ) -> Result<QolipLocation, QolipError> {
        if input.qolip_code.trim().is_empty() || input.size <= 0 {
            let spec = self
                .store
                .product_spec(&input.item_code)
                .await?
                .ok_or(QolipError::MissingQolipCode)?;
            if input.item_name.trim().is_empty() {
                input.item_name = spec.item_name.clone();
            }
            if input.item_group.trim().is_empty() {
                input.item_group = spec.item_group.clone();
            }
            input.qolip_code = spec.qolip_code;
            input.size = spec.size;
        }
        let normalized = normalize_location(input, principal)?;
        self.store.put_location(normalized).await
    }

    pub async fn cell_qr(
        &self,
        input: QolipCellQrInput,
        principal: &Principal,
    ) -> Result<QolipCellQr, QolipError> {
        let normalized = normalize_cell_qr(input, principal)?;
        self.store.get_or_create_cell_qr(normalized).await
    }

    pub async fn location_by_id(
        &self,
        location_id: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        let location_id = location_id.trim();
        if location_id.is_empty() {
            return Err(QolipError::LocationNotFound);
        }
        self.store.location_by_id(location_id).await
    }

    pub async fn location_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        let qolip_code = qolip_code.trim();
        if qolip_code.is_empty() {
            return Err(QolipError::MissingQolipCode);
        }
        self.store.location_by_qolip_code(qolip_code).await
    }

    pub async fn issue_checkout(
        &self,
        input: QolipCheckoutCreate,
        worker_id: &str,
        worker_name: &str,
        principal: &Principal,
    ) -> Result<QolipCheckout, QolipError> {
        let location = self
            .location_by_id(&input.location_id)
            .await?
            .ok_or(QolipError::LocationNotFound)?;
        self.issue_checkout_from_location(
            location,
            input.quantity,
            worker_id,
            worker_name,
            principal,
        )
        .await
    }

    pub async fn issue_checkout_from_location(
        &self,
        location: QolipLocation,
        quantity: i32,
        worker_id: &str,
        worker_name: &str,
        principal: &Principal,
    ) -> Result<QolipCheckout, QolipError> {
        let checkout = normalize_checkout(location, quantity, worker_id, worker_name, principal)?;
        self.store.issue_checkout(checkout).await
    }

    pub async fn checkouts(
        &self,
        _principal: &Principal,
        _is_admin: bool,
        block: Option<&str>,
        status: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        let status = status.trim();
        let status = if status.is_empty() { "open" } else { status };
        let limit = limit.clamp(1, 200);
        if block.is_some() {
            return self.store.checkouts(block, None, status, limit).await;
        }
        self.store.checkouts(None, None, status, limit).await
    }

    pub async fn open_checkouts_for_worker(
        &self,
        worker_refs: &[String],
        worker_name: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        self.store
            .open_checkouts_for_worker(worker_refs, worker_name, limit.clamp(1, 500))
            .await
    }

    pub async fn checkout_by_id(
        &self,
        checkout_id: &str,
    ) -> Result<Option<QolipCheckout>, QolipError> {
        let checkout_id = checkout_id.trim();
        if checkout_id.is_empty() {
            return Ok(None);
        }
        self.store.checkout_by_id(checkout_id).await
    }

    pub async fn return_checkout(
        &self,
        input: QolipCheckoutReturn,
    ) -> Result<QolipCheckout, QolipError> {
        let checkout_id = input.checkout_id.trim();
        if checkout_id.is_empty() {
            return Err(QolipError::CheckoutNotFound);
        }
        self.store
            .return_checkout(checkout_id, &input.row_letter, input.column_number)
            .await
    }

    pub async fn move_location(
        &self,
        input: QolipLocationMove,
        _principal: &Principal,
    ) -> Result<QolipLocation, QolipError> {
        let location_id = input.location_id.trim();
        if location_id.is_empty() {
            return Err(QolipError::LocationNotFound);
        }
        let source = self
            .location_by_id(location_id)
            .await?
            .ok_or(QolipError::LocationNotFound)?;
        let column_number = input.column_number.ok_or(QolipError::InvalidLocation)?;
        let target = normalize_move_target(
            &source,
            &input.block,
            &input.warehouse,
            &input.row_letter,
            column_number,
            input.quantity,
        )?;
        self.store
            .move_location(
                location_id,
                &target.block,
                &target.warehouse,
                &target.row_letter,
                column_number,
                input.quantity,
            )
            .await
    }

    pub async fn move_locations(
        &self,
        inputs: Vec<QolipLocationMove>,
        _principal: &Principal,
    ) -> Result<Vec<QolipLocation>, QolipError> {
        if inputs.is_empty() {
            return Err(QolipError::InvalidLocation);
        }

        let mut seen_location_ids = BTreeSet::new();
        let mut normalized = Vec::with_capacity(inputs.len());
        for input in inputs {
            let location_id = input.location_id.trim();
            if location_id.is_empty() || !seen_location_ids.insert(location_id.to_string()) {
                return Err(QolipError::InvalidLocation);
            }
            let source = self
                .location_by_id(location_id)
                .await?
                .ok_or(QolipError::LocationNotFound)?;
            let column_number = input.column_number.ok_or(QolipError::InvalidLocation)?;
            let target = normalize_move_target(
                &source,
                &input.block,
                &input.warehouse,
                &input.row_letter,
                column_number,
                input.quantity,
            )?;
            normalized.push(QolipLocationMove {
                location_id: location_id.to_string(),
                block: target.block,
                warehouse: target.warehouse,
                quantity: input.quantity,
                row_letter: target.row_letter,
                column_number: target.column_number,
            });
        }

        self.store.move_locations(&normalized).await
    }

    pub async fn cell_qr_by_payload(
        &self,
        qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        let qr_payload = qr_payload.trim();
        if qr_payload.is_empty() {
            return Ok(None);
        }
        self.store.cell_qr_by_payload(qr_payload).await
    }

    pub async fn resolve_cell_qr(
        &self,
        qr_payload: &str,
        principal: &Principal,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        let qr_payload = qr_payload.trim();
        if qr_payload.is_empty() {
            return Ok(None);
        }
        if let Some(cell) = self.store.cell_qr_by_payload(qr_payload).await? {
            return Ok(Some(cell));
        }
        let mut blocks = self.store.assigned_blocks(principal).await?;
        if blocks.is_empty() {
            blocks = self.store.all_blocks().await?;
        }
        let Some(cell) = resolve_cell_qr_from_payload(qr_payload, &blocks, principal) else {
            return Ok(None);
        };
        Ok(Some(self.store.get_or_create_cell_qr(cell).await?))
    }
}
