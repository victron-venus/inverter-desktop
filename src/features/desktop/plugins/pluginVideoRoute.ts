/** Only native-owned windows can resolve opaque media handles. */
export function pluginVideoRoute(search: string, windowLabel: string) {
  const params = new URLSearchParams(search)
  const id = params.get('pluginMedia')
  if (
    !id ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(id) ||
    windowLabel !== `plugin-video-${id}`
  ) {
    return null
  }
  return {
    id,
    name: params.get('name')?.trim() || 'Camera',
    failed: Boolean(params.get('error')),
  }
}
