impl MemoryQolipStore {

    async fn return_checkout(
        &self,
        checkout_id: &str,
        row_letter: &str,
        column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError> {
        let checkout_id = checkout_id.trim();
        let mut checkouts = self.checkouts.write().await;
        let Some(index) = checkouts
            .iter()
            .position(|checkout| checkout.id == checkout_id)
        else {
            return Err(QolipError::CheckoutNotFound);
        };
        if !checkouts[index].status.eq_ignore_ascii_case("open") {
            return Err(QolipError::CheckoutNotReturnable);
        }
        let restore =
            location_from_checkout_target(&checkouts[index], row_letter, column_number)?;
        {
            let mut locations = self.locations.write().await;
            if let Some(target_index) = locations.iter().position(|item| item.id == restore.id) {
                if !location_identity_matches(&locations[target_index], &restore) {
                    return Err(QolipError::LocationIdentityMismatch);
                }
                locations[target_index].quantity += restore.quantity;
            } else {
                locations.push(restore);
            }
            sort_locations(&mut locations);
        }
        checkouts[index].status = "returned".to_string();
        Ok(checkouts[index].clone())
    }

    async fn move_location(
        &self,
        location_id: &str,
        block: &str,
        warehouse: &str,
        row_letter: &str,
        column_number: i32,
        quantity: i32,
    ) -> Result<QolipLocation, QolipError> {
        let input = QolipLocationMove {
            location_id: location_id.to_string(),
            block: block.to_string(),
            warehouse: warehouse.to_string(),
            quantity,
            row_letter: row_letter.to_string(),
            column_number: Some(column_number),
        };
        let mut saved = self.move_locations(&[input]).await?;
        saved.pop().ok_or(QolipError::StoreFailed)
    }

    async fn move_locations(
        &self,
        moves: &[QolipLocationMove],
    ) -> Result<Vec<QolipLocation>, QolipError> {
        let mut locations = self.locations.write().await;
        let mut working = locations.clone();
        let mut saved = Vec::with_capacity(moves.len());
        for input in moves {
            saved.push(apply_memory_location_move(&mut working, input)?);
        }
        sort_locations(&mut working);
        *locations = working;
        Ok(saved)
    }

    async fn cell_qr_by_payload(
        &self,
        qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        let qr_payload = qr_payload.trim();
        Ok(self
            .cell_qrs
            .read()
            .await
            .values()
            .find(|cell| cell.qr_payload.eq_ignore_ascii_case(qr_payload))
            .cloned())
    }
}
