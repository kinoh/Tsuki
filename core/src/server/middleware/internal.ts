import express from 'express'

export function internalOnlyMiddleware(req: express.Request, res: express.Response, next: express.NextFunction): void | express.Response {
  const remoteAddress = req.socket.remoteAddress
  const forwardedFor = req.headers['x-forwarded-for']

  // Get the actual client IP address
  let clientIp = remoteAddress
  if (typeof forwardedFor === 'string' && forwardedFor.trim() !== '') {
    // Use the first IP in X-Forwarded-For header
    clientIp = forwardedFor.split(',')[0].trim()
  }

  // Function to check if IP is in private/local range
  function isPrivateOrLocalIp(ip: string): boolean {
    if (ip.length === 0) {
      return false
    }

    // Remove IPv6-to-IPv4 mapping prefix
    const cleanIp = ip.replace(/^::ffff:/, '')

    // Localhost addresses
    if (cleanIp === '127.0.0.1' || cleanIp === 'localhost' || ip === '::1') {
      return true
    }

    // Check if it's a valid IPv4 address
    const ipv4Regex = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/
    const match = cleanIp.match(ipv4Regex)

    if (match) {
      const octets = match.slice(1).map(Number)

      // RFC 1918 private address ranges:
      // 10.0.0.0/8 (10.0.0.0 - 10.255.255.255)
      if (octets[0] === 10) {
        return true
      }

      // 172.16.0.0/12 (172.16.0.0 - 172.31.255.255)
      if (octets[0] === 172 && octets[1] >= 16 && octets[1] <= 31) {
        return true
      }

      // 192.168.0.0/16 (192.168.0.0 - 192.168.255.255)
      if (octets[0] === 192 && octets[1] === 168) {
        return true
      }

      // Link-local addresses: 169.254.0.0/16
      if (octets[0] === 169 && octets[1] === 254) {
        return true
      }
    }

    // IPv6 local addresses (simplified check)
    if (ip.startsWith('fe80:') || ip.startsWith('fc00:') || ip.startsWith('fd00:')) {
      return true
    }

    return false
  }

  const safeClientIp = clientIp ?? ''
  if (!isPrivateOrLocalIp(safeClientIp)) {
    return res.status(403).json({ error: 'Access denied - internal networks only' })
  }

  next()
}
