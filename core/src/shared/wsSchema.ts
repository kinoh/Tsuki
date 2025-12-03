import { z } from 'zod'

// Generated from api-specs/asyncapi.yaml (manually mirrored until generator wiring is added)
export const clientMessageSchema = z.object({
  type: z.literal('message'),
  text: z.string().optional(),
  images: z.array(z.object({
    data: z.string(),
    mimeType: z.string().optional(),
  })).optional(),
}).refine(
  (msg) => Boolean((msg.text && msg.text.trim() !== '') || (msg.images && msg.images.length > 0)),
  { message: 'Either text or images is required' },
)

export type ClientMessage = z.infer<typeof clientMessageSchema>
