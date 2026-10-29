// Debug script to inspect Better Auth API methods
const { auth } = require('./dist/auth/auth.js');

console.log('Available auth.api methods:');
console.log(Object.keys(auth.api).sort());

// Check for email-related methods
console.log('\nEmail-related methods:');
Object.keys(auth.api).filter(key => key.toLowerCase().includes('email')).forEach(key => {
  console.log(`- ${key}`);
});

// Check for OTP-related methods  
console.log('\nOTP-related methods:');
Object.keys(auth.api).filter(key => key.toLowerCase().includes('otp')).forEach(key => {
  console.log(`- ${key}`);
});

// Check for verification-related methods
console.log('\nVerification-related methods:');
Object.keys(auth.api).filter(key => key.toLowerCase().includes('verif')).forEach(key => {
  console.log(`- ${key}`);
});

// Check for two-factor methods
console.log('\nTwo-factor methods:');
Object.keys(auth.api).filter(key => key.toLowerCase().includes('two') || key.toLowerCase().includes('totp') || key.toLowerCase().includes('factor')).forEach(key => {
  console.log(`- ${key}`);
});

// Check for phone methods
console.log('\nPhone-related methods:');
Object.keys(auth.api).filter(key => key.toLowerCase().includes('phone')).forEach(key => {
  console.log(`- ${key}`);
});

// Check for passkey methods
console.log('\nPasskey-related methods:');
Object.keys(auth.api).filter(key => key.toLowerCase().includes('passkey')).forEach(key => {
  console.log(`- ${key}`);
});