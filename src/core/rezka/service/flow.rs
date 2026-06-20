use super::*;

impl RezkaService {
    pub async fn split(
        &self,
        source: RezkaSourceEntry,
        request: RezkaSplitRequest,
    ) -> Result<RezkaSplitResponse, RezkaServiceError> {
        let repack_store = self.repack_store.as_ref().ok_or_else(|| {
            RezkaServiceError::NotConfigured("rezka repack store is not configured".into())
        })?;
        let driver = self.driver.as_ref().ok_or_else(|| {
            RezkaServiceError::NotConfigured("scale driver is not configured".into())
        })?;
        let job = NormalizedRezkaSplit::from_request(source, request, self.epc.as_deref())?;
        tracing::info!(
            source_barcode = %job.source.barcode,
            source_item_code = %job.source.item_code,
            source_qty = job.source.qty,
            output_count = job.outputs.len(),
            printable_count = job.printable_outputs.len(),
            outputs = ?rezka_output_log(&job.outputs),
            printable_outputs = ?rezka_output_log(&job.printable_outputs),
            "rezka split normalized"
        );
        let draft = repack_store
            .create_rezka_repack_draft(CreateRezkaRepackDraftInput {
                source: job.source.clone(),
                reason: job.reason.clone(),
                outputs: job.outputs.clone(),
            })
            .await
            .map_err(|error| RezkaServiceError::StoreWrite(error.message()))?;

        for output in &job.printable_outputs {
            tracing::info!(
                stock_entry_name = %draft.name,
                epc = %output.epc,
                item_code = %output.item_code,
                item_name = %output.item_name,
                qty = output.qty,
                uom = %output.uom,
                warehouse = %output.warehouse,
                reason = %output.reason,
                print_qr = output.print_qr,
                "rezka split sending print request"
            );
            let print = driver
                .print_material_receipt(job.driver_request(output))
                .await;
            match print {
                Ok(print) if print_done(&print) => {
                    tracing::info!(
                        stock_entry_name = %draft.name,
                        epc = %output.epc,
                        item_code = %output.item_code,
                        qty = output.qty,
                        printer = %print.printer,
                        mode = %print.mode,
                        status = %print.status,
                        "rezka split print done"
                    );
                }
                Ok(print) => {
                    tracing::warn!(
                        stock_entry_name = %draft.name,
                        epc = %output.epc,
                        item_code = %output.item_code,
                        qty = output.qty,
                        status = %print.status,
                        detail = %print_error_detail(&print),
                        "rezka split print failed"
                    );
                    let _ = repack_store.delete_rezka_repack_draft(&draft.name).await;
                    return Err(RezkaServiceError::PrintFailed(print_error_detail(&print)));
                }
                Err(error) => {
                    tracing::warn!(
                        stock_entry_name = %draft.name,
                        epc = %output.epc,
                        item_code = %output.item_code,
                        qty = output.qty,
                        error = %error.message(),
                        "rezka split print request error"
                    );
                    let _ = repack_store.delete_rezka_repack_draft(&draft.name).await;
                    return Err(RezkaServiceError::PrintFailed(error.message()));
                }
            }
        }

        repack_store
            .submit_rezka_repack_draft(&draft.name)
            .await
            .map_err(|error| {
                RezkaServiceError::SubmitFailed(clean_store_error(&error.message()))
            })?;

        Ok(RezkaSplitResponse {
            ok: true,
            status: "printed".to_string(),
            stock_entry_name: draft.name,
            source_barcode: job.source.barcode,
            outputs: job.printable_outputs,
        })
    }
}
