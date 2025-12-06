import { z } from 'zod'

// Generated from api-specs/asyncapi.yaml (manually mirrored until generator wiring is added)
export const clientChatMessageSchema = z.object({
  type: z.literal('message'),
  text: z.string().optional(),
  images: z.array(z.object({
    data: z.string(),
    mimeType: z.string().optional(),
  })).optional(),
}).refine(
  (msg) => {
    const hasText = typeof msg.text === 'string' && msg.text.trim() !== ''
    const hasImages = Array.isArray(msg.images) && msg.images.length > 0
    return hasText || hasImages
  },
  { message: 'Either text or images is required' },
)

export const clientSensoryMessageSchema = z.object({
  type: z.literal('sensory'),
  text: z.string().min(1),
})

export const clientMessageSchema = z.union([clientChatMessageSchema, clientSensoryMessageSchema])

export type ClientMessage = z.infer<typeof clientMessageSchema>
