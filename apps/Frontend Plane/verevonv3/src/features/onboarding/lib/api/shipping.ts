import { requestJson } from '@/shared/api/http'
import { parseOnboardingResponse, shippingCarriersResponseSchema } from './response-schemas'

export type ShippingCarrier = {
  name: string
  is_mock?: boolean
}

export async function getShippingCarriers(): Promise<ShippingCarrier[]> {
  const endpoint = '/api/v1/shipping/carriers'
  const response = parseOnboardingResponse(
    shippingCarriersResponseSchema,
    await requestJson<unknown>(endpoint),
    endpoint,
  )
  return response.carriers
}
