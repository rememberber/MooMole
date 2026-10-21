import catalog from './maintenance-catalog.json'

// Source-bound reference information, never a machine assessment or execution plan.
export const maintenanceCatalog = catalog
export type MaintenanceItem = (typeof catalog.items)[number]

export function maintenanceItems(query: string): MaintenanceItem[] {
  const needle = query.trim().toLocaleLowerCase()
  return catalog.items.filter((item) =>
    Object.values(item).some((value) => value.toLocaleLowerCase().includes(needle)),
  )
}

export function maintenanceSourceMatches(revision: string, version: string | null): boolean {
  return revision === catalog.revision && version === catalog.version
}
