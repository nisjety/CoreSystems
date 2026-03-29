'use client';

import React, { forwardRef, useState } from 'react';
import { Eye, EyeOff, AlertCircle } from 'lucide-react';
import { useLanguageSwitch } from '../lib/i18n/hooks';

interface FormFieldProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'size'> {
  label: string;
  error?: string;
  hint?: string;
  isPassword?: boolean;
  showPasswordToggle?: boolean;
  leftIcon?: React.ReactNode;
  rightIcon?: React.ReactNode;
  containerClassName?: string;
  labelClassName?: string;
  inputClassName?: string;
  errorClassName?: string;
  hintClassName?: string;
  language?: 'no' | 'en';
}

export const FormField = forwardRef<HTMLInputElement, FormFieldProps>(({
  label,
  error,
  hint,
  isPassword = false,
  showPasswordToggle = false,
  leftIcon,
  rightIcon,
  containerClassName = '',
  labelClassName = '',
  inputClassName = '',
  errorClassName = '',
  hintClassName = '',
  className = '',
  type = 'text',
  id,
  required,
  language,
  ...props
}, ref) => {
  // Use i18n hook for language detection
  const { isNorwegian } = useLanguageSwitch();
  const currentLanguage = language || (isNorwegian ? 'no' : 'en');

  const [showPassword, setShowPassword] = useState(false);
  
  const fieldId = id || `field-${label.toLowerCase().replace(/\s+/g, '-')}`;
  const inputType = isPassword ? (showPassword ? 'text' : 'password') : type;
  const hasError = !!error;
  const hasIcons = leftIcon || rightIcon || (isPassword && showPasswordToggle);

  // Localized accessibility texts
  const a11yTexts = {
    no: {
      showPassword: 'Vis passord',
      hidePassword: 'Skjul passord',
    },
    en: {
      showPassword: 'Show password',
      hidePassword: 'Hide password',
    },
  };

  const texts = a11yTexts[currentLanguage];

  const togglePasswordVisibility = () => {
    setShowPassword(!showPassword);
  };

  return (
    <div className={`form-field ${containerClassName}`}>
      <label 
        htmlFor={fieldId}
        className={`block text-sm font-medium text-foreground mb-1 ${labelClassName}`}
      >
        {label}
        {required && <span className="text-destructive ml-1">*</span>}
      </label>
      
      {hint && (
        <p className={`text-xs text-muted-foreground mb-2 ${hintClassName}`}>
          {hint}
        </p>
      )}
      
      <div className="form-field__input-container relative">
        {leftIcon && (
          <div className="absolute left-3 top-1/2 transform -translate-y-1/2 text-muted-foreground">
            {leftIcon}
          </div>
        )}
        
        <input
          ref={ref}
          id={fieldId}
          type={inputType}
          className={`
            w-full px-3 py-2 border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring 
            text-sm bg-background text-foreground transition-colors
            placeholder-muted-foreground
            disabled:bg-muted disabled:text-muted-foreground disabled:cursor-not-allowed
            ${hasError 
              ? 'border-destructive focus:border-destructive focus:ring-destructive/50' 
              : 'border-border focus:border-ring'
            }
            ${leftIcon ? 'pl-10' : ''}
            ${hasIcons && !leftIcon ? 'pr-10' : ''}
            ${inputClassName}
            ${className}
          `}
          aria-invalid={hasError}
          aria-describedby={
            error ? `${fieldId}-error` : 
            hint ? `${fieldId}-hint` : undefined
          }
          {...props}
        />
        
        {isPassword && showPasswordToggle && (
          <button
            type="button"
            onClick={togglePasswordVisibility}
            className="absolute right-3 top-1/2 transform -translate-y-1/2 text-muted-foreground hover:text-foreground focus:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 rounded transition-colors"
            aria-label={showPassword ? texts.hidePassword : texts.showPassword}
            tabIndex={-1}
          >
            {showPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
          </button>
        )}
        
        {rightIcon && !showPasswordToggle && (
          <div className="absolute right-3 top-1/2 transform -translate-y-1/2 text-muted-foreground">
            {rightIcon}
          </div>
        )}
      </div>
      
      {error && (
        <div 
          id={`${fieldId}-error`}
          className={`mt-1 flex items-center text-sm text-destructive ${errorClassName}`}
          role="alert"
        >
          <AlertCircle className="w-4 h-4 mr-1 flex-shrink-0" />
          {error}
        </div>
      )}
    </div>
  );
});

FormField.displayName = 'FormField';

export default FormField;