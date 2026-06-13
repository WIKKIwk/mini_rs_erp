use super::*;

const ADMIN_ONLY: &[PrincipalRole] = &[PrincipalRole::Admin];
const WERKA_ONLY: &[PrincipalRole] = &[PrincipalRole::Werka];
const SUPPLIER_ONLY: &[PrincipalRole] = &[PrincipalRole::Supplier];
const CUSTOMER_ONLY: &[PrincipalRole] = &[PrincipalRole::Customer];
const SUPPLIER_WERKA: &[PrincipalRole] = &[PrincipalRole::Supplier, PrincipalRole::Werka];
const ADMIN_WERKA: &[PrincipalRole] = &[PrincipalRole::Admin, PrincipalRole::Werka];

pub(super) const CAPABILITY_CATALOG: &[CapabilityDefinition] = &[
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
        default_roles: ADMIN_ONLY,
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
        capability: Capability::PushTokenManage,
        code: "push.token.manage",
        label: "Push token manage",
        default_roles: SUPPLIER_WERKA,
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
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemCreate,
        code: "catalog.item.create",
        label: "Catalog item create",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemGroupRead,
        code: "catalog.item_group.read",
        label: "Catalog item group read",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemGroupManage,
        code: "catalog.item_group.manage",
        label: "Catalog item group manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemBulkMove,
        code: "catalog.item.bulk_move",
        label: "Catalog item bulk move",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::SupplierDirectoryRead,
        code: "party.supplier.read",
        label: "Supplier directory read",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::SupplierDirectoryManage,
        code: "party.supplier.manage",
        label: "Supplier directory manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::SupplierItemAssign,
        code: "party.supplier.item.assign",
        label: "Supplier item assign",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::SupplierCodeManage,
        code: "party.supplier.code.manage",
        label: "Supplier code manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CustomerDirectoryRead,
        code: "party.customer.read",
        label: "Customer directory read",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CustomerDirectoryManage,
        code: "party.customer.manage",
        label: "Customer directory manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CustomerItemAssign,
        code: "party.customer.item.assign",
        label: "Customer item assign",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CustomerCodeManage,
        code: "party.customer.code.manage",
        label: "Customer code manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::AdminActivityRead,
        code: "admin.activity.read",
        label: "Admin activity read",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::WerkaCodeManage,
        code: "werka.code.manage",
        label: "Werka code manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::ProductionMapManage,
        code: "production.map.manage",
        label: "Production map manage",
        default_roles: ADMIN_ONLY,
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
        default_roles: ADMIN_WERKA,
    },
    CapabilityDefinition {
        capability: Capability::GscalePrint,
        code: "gscale.print",
        label: "GScale print",
        default_roles: ADMIN_WERKA,
    },
    CapabilityDefinition {
        capability: Capability::RpsBatchManage,
        code: "rps.batch.manage",
        label: "RPS batch manage",
        default_roles: ADMIN_WERKA,
    },
    CapabilityDefinition {
        capability: Capability::RezkaSplitManage,
        code: "rezka.split.manage",
        label: "Rezka split manage",
        default_roles: ADMIN_ONLY,
    },
];
