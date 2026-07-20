use super::super::*;
use super::admin_read::FakeAdminReadPort;

#[async_trait]
impl AdminWritePort for FakeAdminReadPort {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry("SUP-NEW", name, phone))
    }

    async fn update_supplier_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn assign_supplier_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn unassign_supplier_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry("CUST-NEW", name, phone))
    }

    async fn update_customer_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn update_customer_code(&self, _ref_: &str, _code: &str) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn create_material_taminotchi(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry("MAT-NEW", name, phone))
    }

    async fn update_material_taminotchi_phone(
        &self,
        _ref_: &str,
        _phone: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn update_material_taminotchi_code(
        &self,
        _ref_: &str,
        _code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn assign_customer_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn unassign_customer_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        if code.eq_ignore_ascii_case("ITEM-DUPLICATE") {
            return Err(AdminPortError::InvalidInput(
                "item code already exists".to_string(),
            ));
        }
        Ok(SupplierItem {
            code: code.to_string(),
            name: name.to_string(),
            uom: uom.to_string(),
            warehouse: String::new(),
            item_group: item_group.to_string(),
            customer_names: Vec::new(),
        })
    }

    async fn update_item(
        &self,
        _original_code: &str,
        code: &str,
        name: &str,
    ) -> Result<AdminItemDetail, AdminPortError> {
        Ok(AdminItemDetail {
            code: code.trim().to_string(),
            name: name.trim().to_string(),
            uom: "Kg".to_string(),
            item_group: "Tayyor mahsulot".to_string(),
            is_finished_goods: true,
            created_at_unix: 1_720_000_000,
            updated_at_unix: 1_720_000_200,
            customers: vec![CustomerDirectoryEntry {
                ref_: "CUST-001".to_string(),
                name: "Customer One".to_string(),
                phone: "+998904444444".to_string(),
            }],
        })
    }

    async fn delete_item(&self, code: &str) -> Result<(), AdminPortError> {
        if code.eq_ignore_ascii_case("ITEM-ACTIVE") {
            return Err(AdminPortError::InvalidInput(
                "item is used by active order".to_string(),
            ));
        }
        Ok(())
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        Ok(AdminItemGroup {
            name: name.trim().to_string(),
            item_group_name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group,
        })
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        Ok(AdminItemGroup {
            name: name.trim().to_string(),
            item_group_name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group: true,
        })
    }

    async fn update_item_group(
        &self,
        _item_code: &str,
        _item_group: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }
}

pub(crate) struct MissingSupplierWritePort;

#[async_trait]
impl AdminWritePort for MissingSupplierWritePort {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.create_supplier(name, phone).await
    }

    async fn update_supplier_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::NotFound)
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .assign_supplier_item(ref_, item_code)
            .await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .unassign_supplier_item(ref_, item_code)
            .await
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.create_customer(name, phone).await
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        FakeAdminReadPort.update_customer_phone(ref_, phone).await
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        FakeAdminReadPort.update_customer_code(ref_, code).await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .assign_customer_item(ref_, item_code)
            .await
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .unassign_customer_item(ref_, item_code)
            .await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        FakeAdminReadPort
            .create_item(code, name, uom, item_group)
            .await
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        FakeAdminReadPort
            .create_item_group(name, parent, is_group)
            .await
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        FakeAdminReadPort.move_item_group_parent(name, parent).await
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .update_item_group(item_code, item_group)
            .await
    }
}

#[derive(Default)]
pub(crate) struct CountingSupplierWritePort {
    pub(crate) supplier_phone_updates: AtomicUsize,
}

#[async_trait]
impl AdminWritePort for CountingSupplierWritePort {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.create_supplier(name, phone).await
    }

    async fn update_supplier_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        self.supplier_phone_updates.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .assign_supplier_item(ref_, item_code)
            .await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .unassign_supplier_item(ref_, item_code)
            .await
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.create_customer(name, phone).await
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        FakeAdminReadPort.update_customer_phone(ref_, phone).await
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        FakeAdminReadPort.update_customer_code(ref_, code).await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .assign_customer_item(ref_, item_code)
            .await
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .unassign_customer_item(ref_, item_code)
            .await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        FakeAdminReadPort
            .create_item(code, name, uom, item_group)
            .await
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        FakeAdminReadPort
            .create_item_group(name, parent, is_group)
            .await
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        FakeAdminReadPort.move_item_group_parent(name, parent).await
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .update_item_group(item_code, item_group)
            .await
    }
}
