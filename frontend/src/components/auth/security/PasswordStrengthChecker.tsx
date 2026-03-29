'use client';

import React, { useState } from 'react';
import { Eye, EyeOff, Shield, AlertTriangle, Check, X } from 'lucide-react';
import { usePasswordStrengthAnalysis } from '../lib/api/auth-provider-hooks';
import { Input } from '../ui/input';
import { Button } from '../ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../ui/card';

interface PasswordStrengthCheckerProps {
  value?: string;
  onChange?: (value: string) => void;
  onValidationChange?: (isValid: boolean) => void;
  label?: string;
  placeholder?: string;
  className?: string;
  disabled?: boolean;
}

export function PasswordStrengthChecker({
  value = '',
  onChange,
  onValidationChange,
  label = 'Password',
  placeholder = 'Enter your password',
  className = '',
  disabled = false,
}: PasswordStrengthCheckerProps) {
  const [showPassword, setShowPassword] = useState(false);
  const [password, setPassword] = useState(value);

  // Use the password strength analysis hook with debouncing
  const {
    data: strengthData,
    isLoading,
    isError,
  } = usePasswordStrengthAnalysis(password, password.length >= 3);

  const handlePasswordChange = (newPassword: string) => {
    setPassword(newPassword);
    onChange?.(newPassword);
    
    // Notify parent about validation status
    if (strengthData && !isLoading) {
      const isValid = strengthData.isStrong && !strengthData.isCompromised;
      onValidationChange?.(isValid);
    }
  };

  const getStrengthColor = (score: number) => {
    if (score <= 1) return 'text-red-500';
    if (score <= 2) return 'text-orange-500';
    if (score <= 3) return 'text-yellow-500';
    return 'text-green-500';
  };

  const getStrengthLabel = (score: number) => {
    if (score <= 1) return 'Very Weak';
    if (score <= 2) return 'Weak';
    if (score <= 3) return 'Good';
    return 'Strong';
  };

  const getStrengthBarWidth = (score: number) => {
    return `${Math.max(10, (score / 4) * 100)}%`;
  };

  return (
    <div className={`space-y-3 ${className}`}>
      {/* Password Input */}
      <div className="space-y-2">
        <label htmlFor="password" className="text-sm font-medium text-gray-700 dark:text-gray-300">
          {label}
        </label>
        <div className="relative">
          <Input
            id="password"
            type={showPassword ? 'text' : 'password'}
            value={password}
            onChange={(e) => handlePasswordChange(e.target.value)}
            placeholder={placeholder}
            disabled={disabled}
            className="pr-10"
          />
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="absolute right-0 top-0 h-full px-3 py-2 hover:bg-transparent"
            onClick={() => setShowPassword(!showPassword)}
            disabled={disabled}
          >
            {showPassword ? (
              <EyeOff className="h-4 w-4 text-gray-400" />
            ) : (
              <Eye className="h-4 w-4 text-gray-400" />
            )}
          </Button>
        </div>
      </div>

      {/* Password Strength Indicator */}
      {password.length >= 3 && (
        <Card className="border-gray-200 dark:border-gray-700">
          <CardHeader className="pb-3">
            <CardTitle className="text-sm flex items-center gap-2">
              <Shield className="h-4 w-4" />
              Password Strength
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            {isLoading ? (
              <div className="flex items-center gap-2 text-sm text-gray-500">
                <div className="w-4 h-4 border-2 border-gray-300 border-t-blue-500 rounded-full animate-spin" />
                Checking password strength...
              </div>
            ) : isError ? (
              <div className="flex items-center gap-2 text-sm text-red-500">
                <X className="h-4 w-4" />
                Could not check password strength
              </div>
            ) : strengthData ? (
              <>
                {/* Strength Bar */}
                <div className="space-y-2">
                  <div className="flex justify-between items-center">
                    <span className={`text-sm font-medium ${getStrengthColor(strengthData.score)}`}>
                      {getStrengthLabel(strengthData.score)}
                    </span>
                    {strengthData.estimatedCrackTime && (
                      <span className="text-xs text-gray-500">
                        Crack time: {strengthData.estimatedCrackTime}
                      </span>
                    )}
                  </div>
                  <div className="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-2">
                    <div
                      className={`h-2 rounded-full transition-all duration-300 ${
                        strengthData.score <= 1
                          ? 'bg-red-500'
                          : strengthData.score <= 2
                          ? 'bg-orange-500'
                          : strengthData.score <= 3
                          ? 'bg-yellow-500'
                          : 'bg-green-500'
                      }`}
                      style={{ width: getStrengthBarWidth(strengthData.score) }}
                    />
                  </div>
                </div>

                {/* Compromise Warning */}
                {strengthData.isCompromised && (
                  <div className="flex items-start gap-2 p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-md">
                    <AlertTriangle className="h-4 w-4 text-red-500 mt-0.5 flex-shrink-0" />
                    <div className="text-sm">
                      <p className="font-medium text-red-800 dark:text-red-200">
                        Password Compromised
                      </p>
                      <p className="text-red-700 dark:text-red-300 mt-1">
                        This password has been found in data breaches. Please choose a different password.
                      </p>
                    </div>
                  </div>
                )}

                {/* Feedback */}
                {strengthData.feedback && strengthData.feedback.length > 0 && (
                  <div className="space-y-1">
                    <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300">
                      Suggestions:
                    </h4>
                    <ul className="space-y-1">
                      {strengthData.feedback.map((feedback, index) => (
                        <li key={index} className="flex items-start gap-2 text-sm text-gray-600 dark:text-gray-400">
                          <span className="text-orange-500 mt-0.5">•</span>
                          {feedback}
                        </li>
                      ))}
                    </ul>
                  </div>
                )}

                {/* Success State */}
                {strengthData.isStrong && !strengthData.isCompromised && (
                  <div className="flex items-center gap-2 text-sm text-green-600 dark:text-green-400">
                    <Check className="h-4 w-4" />
                    Password meets security requirements
                  </div>
                )}
              </>
            ) : null}
          </CardContent>
        </Card>
      )}
    </div>
  );
}

export default PasswordStrengthChecker;