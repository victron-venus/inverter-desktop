/** Only native-owned windows can resolve opaque media handles. */
export function pluginVideoRoute(search: string, windowLabel: string) {
  const params = new URLSearchParams(search)
  const id = params.get('pluginMedia')
  const mediaKind = params.get('pluginMediaKind') ?? 'video'
  if (
    !id ||
    (mediaKind !== 'video' && mediaKind !== 'image' && mediaKind !== 'live') ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(id) ||
    windowLabel !== `plugin-${mediaKind === 'live' ? 'preview' : 'video'}-${id}`
  ) {
    return null
  }
  return {
    id,
    mediaKind,
    name: params.get('name')?.trim() || 'Camera',
    failed: Boolean(params.get('error')),
  } as const
}
