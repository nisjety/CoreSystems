export function clearAllCookies(): void {
  document.cookie.split(";").forEach(function(c) { 
    document.cookie = c.replace(/^ +/, "").replace(/=.*/, "=;expires=" + new Date().toUTCString() + ";path=/"); 
  });
}

export function clearAllStorage(): void {
  localStorage.clear();
  sessionStorage.clear();
}

export function clearAllAuthData(): void {
  clearAllCookies();
  clearAllStorage();
}
