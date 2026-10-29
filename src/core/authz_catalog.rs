use super::*;
use crate::core::auth::models::PrincipalRole;

const ADMIN_BRIGADER: &[PrincipalRole] = &[PrincipalRole::Admin, PrincipalRole::Brigader];
const ADMIN_SALES_BRIGADER: &[PrincipalRole] = &[
    PrincipalRole::Admin,
    PrincipalRole::SalesManager,
    PrincipalRole::Brigader,
];
const ADMIN_APARATCHI_BRIGADER: &[PrincipalRole] = &[
    PrincipalRole::Admin,
    PrincipalRole::Aparatchi,
    PrincipalRole::Brigader,
];
/// Operatsion kirim/ulash to'plami: tarozi katalogi, kirim chop etish,
/// RPS batch va homashyoni orderga ulash. Tayyorlov masteri ham o'z
/// ombori + biriktirilgan homashyolari doirasida shu to'plamni ishlatadi.
const ADMIN_WERKA_MATERIAL_TAYYORLOV_BRIGADER_SCOPE: &[PrincipalRole] = &[
    PrincipalRole::Admin,
    PrincipalRole::Werka,
    PrincipalRole::MaterialTaminotchi,
    PrincipalRole::TayyorlovMasteri,
    PrincipalRole::Brigader,
];
const ADMIN_ONLY: &[PrincipalRole] = &[PrincipalRole::Admin];
const WERKA_ONLY: &[PrincipalRole] = &[PrincipalRole::Werka];
const SUPPLIER_ONLY: &[PrincipalRole] = &[PrincipalRole::Supplier];
const CUSTOMER_ONLY: &[PrincipalRole] = &[PrincipalRole::Customer];
const QOLIPCHI_ONLY: &[PrincipalRole] = &[PrincipalRole::Qolipchi];
const BOYOQCHI_ONLY: &[PrincipalRole] = &[PrincipalRole::Boyoqchi];
const INVENTORY_MOVEMENT_ROLES: &[PrincipalRole] = &[
    PrincipalRole::Brigader,
    PrincipalRole::Admin,
    PrincipalRole::Werka,
    PrincipalRole::MaterialTaminotchi,
    PrincipalRole::Qolipchi,
    PrincipalRole::Aparatchi,
];

pub(super) const CAPABILITY_CATALOG: &[CapabilityDefinition] = &[
    CapabilityDefinition {
        capability: Capability::BrigaderAccess,
        code: "brigader.access",
        label: "Brigader ish joyi",
        default_roles: &[PrincipalRole::Brigader],
    },
    CapabilityDefinition {
        capability: Capability::SalesAccess,
        code: "sales.access",
        label: "Sotuv manageri ish joyi",
        default_roles: &[PrincipalRole::SalesManager],
    },
    CapabilityDefinition {
        capability: Capability::ProductionMapRead,
        code: "production.map.read",
        label: "Production map read",
        default_roles: ADMIN_SALES_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::ProductionOrderCreate,
        code: "production.order.create",
        label: "Production order create",
        default_roles: ADMIN_SALES_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::ProductionOrderEdit,
        code: "production.order.edit",
        label: "Production order edit",
        default_roles: ADMIN_SALES_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::ProductionMapEdit,
        code: "production.map.edit",
        label: "Production map edit",
        default_roles: ADMIN_SALES_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::RawMaterialSplitAccess,
        code: "raw_material.split",
        label: "Homashyo rezkachisi ish joyi",
        default_roles: &[PrincipalRole::HomashyoRezkachi],
    },
    CapabilityDefinition {
        capability: Capability::PreparationAccess,
        code: "preparation.access",
        label: "Tayyorlov masteri ish joyi",
        default_roles: &[PrincipalRole::TayyorlovMasteri],
    },
    CapabilityDefinition {
        capability: Capability::AdminAccess,
        code: "admin.access",
        label: "Admin panel",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::RoleCapabilityRead,
        code: "role.capability.read",
        label: "Role capability catalog read",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::RoleCapabilityManage,
        code: "role.capability.manage",
        label: "Role capability manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::AdminSettingsRead,
        code: "admin.settings.read",
        label: "Admin settings read",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::AdminSettingsManage,
        code: "admin.settings.manage",
        label: "Admin settings manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::WerkaAccess,
        code: "werka.access",
        label: "Werka workspace",
        default_roles: WERKA_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::SupplierAccess,
        code: "supplier.access",
        label: "Supplier workspace",
        default_roles: SUPPLIER_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CustomerAccess,
        code: "customer.access",
        label: "Customer workspace",
        default_roles: CUSTOMER_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::BoyoqchiAccess,
        code: "boyoqchi.access",
        label: "Bo‘yoqchi ish joyi",
        default_roles: BOYOQCHI_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::ReturnedPaintRequestCreate,
        code: "returned_paint.request.create",
        label: "Qaytarilgan bo‘yoq ma’lumotini yuborish",
        default_roles: ADMIN_APARATCHI_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::ReturnedPaintRequestRead,
        code: "returned_paint.request.read",
        label: "Qaytarilgan bo‘yoq ma’lumotlarini ko‘rish",
        default_roles: BOYOQCHI_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::PushTokenManage,
        code: "push.token.manage",
        label: "Push token manage",
        default_roles: &[
            PrincipalRole::Brigader,
            PrincipalRole::Supplier,
            PrincipalRole::Werka,
            PrincipalRole::SalesManager,
        ],
    },
    CapabilityDefinition {
        capability: Capability::SupplierAvatarManage,
        code: "supplier.avatar.manage",
        label: "Supplier avatar manage",
        default_roles: SUPPLIER_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemRead,
        code: "catalog.item.read",
        label: "Catalog item read",
        default_roles: ADMIN_SALES_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemCreate,
        code: "catalog.item.create",
        label: "Catalog item create",
        default_roles: &[
            PrincipalRole::Brigader,
            PrincipalRole::Admin,
            PrincipalRole::MaterialTaminotchi,
            PrincipalRole::SalesManager,
        ],
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemGroupRead,
        code: "catalog.item_group.read",
        label: "Catalog item group read",
        default_roles: ADMIN_SALES_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemGroupManage,
        code: "catalog.item_group.manage",
        label: "Catalog item group manage",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemBulkMove,
        code: "catalog.item.bulk_move",
        label: "Catalog item bulk move",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::SupplierDirectoryRead,
        code: "party.supplier.read",
        label: "Supplier directory read",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::SupplierDirectoryManage,
        code: "party.supplier.manage",
        label: "Supplier directory manage",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::SupplierItemAssign,
        code: "party.supplier.item.assign",
        label: "Supplier item assign",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::SupplierCodeManage,
        code: "party.supplier.code.manage",
        label: "Supplier code manage",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::CustomerDirectoryRead,
        code: "party.customer.read",
        label: "Customer directory read",
        default_roles: ADMIN_SALES_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::CustomerDirectoryManage,
        code: "party.customer.manage",
        label: "Customer directory manage",
        default_roles: ADMIN_SALES_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::CustomerItemAssign,
        code: "party.customer.item.assign",
        label: "Customer item assign",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::CustomerCodeManage,
        code: "party.customer.code.manage",
        label: "Customer code manage",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::AdminActivityRead,
        code: "admin.activity.read",
        label: "Admin activity read",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::WerkaCodeManage,
        code: "werka.code.manage",
        label: "Werka code manage",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::ProductionMapManage,
        code: "production.map.manage",
        label: "Production map manage",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::FactoryLocationManage,
        code: "factory.location.manage",
        label: "Factory location manage",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::InventoryMovementManage,
        code: "inventory.movement.manage",
        label: "Inventory location and warehouse transfer manage",
        default_roles: INVENTORY_MOVEMENT_ROLES,
    },
    CapabilityDefinition {
        capability: Capability::ApparatusQueueRead,
        code: "apparatus.queue.read",
        label: "Apparatus queue read",
        default_roles: &[],
    },
    CapabilityDefinition {
        capability: Capability::ApparatusQueueManage,
        code: "apparatus.queue.manage",
        label: "Apparatus queue manage",
        default_roles: &[],
    },
    CapabilityDefinition {
        capability: Capability::GscaleCatalogRead,
        code: "gscale.catalog.read",
        label: "GScale catalog read",
        default_roles: ADMIN_WERKA_MATERIAL_TAYYORLOV_BRIGADER_SCOPE,
    },
    CapabilityDefinition {
        capability: Capability::GscalePrint,
        code: "gscale.print",
        label: "GScale print",
        default_roles: ADMIN_WERKA_MATERIAL_TAYYORLOV_BRIGADER_SCOPE,
    },
    CapabilityDefinition {
        capability: Capability::RpsBatchManage,
        code: "rps.batch.manage",
        label: "RPS batch manage",
        default_roles: ADMIN_WERKA_MATERIAL_TAYYORLOV_BRIGADER_SCOPE,
    },
    CapabilityDefinition {
        capability: Capability::RezkaSplitManage,
        code: "rezka.split.manage",
        label: "Rezka split manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::QolipManage,
        code: "qolip.manage",
        label: "Qolip ombor manage",
        default_roles: QOLIPCHI_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::RawMaterialRuleManage,
        code: "raw_material.rule.manage",
        label: "Raw material rule manage",
        default_roles: ADMIN_BRIGADER,
    },
    CapabilityDefinition {
        capability: Capability::RawMaterialAssign,
        code: "raw_material.assign",
        label: "Raw material assign",
        default_roles: ADMIN_WERKA_MATERIAL_TAYYORLOV_BRIGADER_SCOPE,
    },
];
