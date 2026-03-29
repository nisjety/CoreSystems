export async function checkPasskeySupport(): Promise<boolean> {
  if (typeof window === 'undefined' || !window.PublicKeyCredential) {
    return false;
  }

  try {
    const isSupported = await PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable();
    const isConditionalUISupported = PublicKeyCredential.isConditionalMediationAvailable ? 
      await PublicKeyCredential.isConditionalMediationAvailable() : false;
    
    return isSupported || isConditionalUISupported;
  } catch (error) {
    console.debug('Passkey support check failed:', error);
    return false;
  }
}

export function getUserDeviceType(): 'mobile' | 'desktop' | 'tablet' {
  if (typeof window === 'undefined') return 'desktop';
  
  const userAgent = navigator.userAgent.toLowerCase();
  
  if (/mobile|android|iphone|ipod|blackberry|iemobile|opera mini/i.test(userAgent)) {
    return 'mobile';
  }
  
  if (/tablet|ipad/i.test(userAgent)) {
    return 'tablet';
  }
  
  return 'desktop';
}

export function getDeviceName(): string {
  const deviceType = getUserDeviceType();
  const platform = navigator.platform || 'Unknown';
  
  return `${platform} ${deviceType}`;
}
