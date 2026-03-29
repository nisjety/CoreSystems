'use client';

/**
 * Example showing how to use the centralized ModelSelector
 * This file demonstrates the new centralized approach for model selection
 */

import { useState } from 'react';
import { useI18n } from '@/components/chat/hooks/i18n';
import { 
  ModelSelector, 
  getDefaultModel, 
  getModelNames, 
  getModelDescription,
  AVAILABLE_MODELS 
} from './ModelSelector';

export function ModelSelectorExample() {
  const { t } = useI18n();
  
  // ✅ CORRECT: Use getDefaultModel for initial state
  const [selectedModel, setSelectedModel] = useState(getDefaultModel(t));
  
  // ✅ CORRECT: Get all available model names for dropdowns/lists
  const availableModels = getModelNames(t);
  
  return (
    <div className="p-6 space-y-6">
      <h2 className="text-xl font-semibold">Centralized Model Selection Examples</h2>
      
      {/* Example 1: Using ModelSelector component */}
      <div className="border rounded-lg p-4">
        <h3 className="font-medium mb-3">1. Using ModelSelector Component</h3>
        <ModelSelector
          value={selectedModel}
          onChange={setSelectedModel}
        />
        <p className="text-sm text-gray-600 mt-2">
          Selected: {selectedModel}
        </p>
      </div>
      
      {/* Example 2: Getting model information */}
      <div className="border rounded-lg p-4">
        <h3 className="font-medium mb-3">2. Model Information</h3>
        <div className="space-y-2">
          <p><strong>Default model:</strong> {getDefaultModel(t)}</p>
          <p><strong>Description:</strong> {getModelDescription(selectedModel, t)}</p>
          <p><strong>Available models:</strong> {availableModels.join(', ')}</p>
          <p><strong>Total models:</strong> {AVAILABLE_MODELS.length}</p>
        </div>
      </div>
      
      {/* Example 3: Custom dropdown using centralized data */}
      <div className="border rounded-lg p-4">
        <h3 className="font-medium mb-3">3. Custom Dropdown</h3>
        <select 
          value={selectedModel} 
          onChange={(e) => setSelectedModel(e.target.value)}
          className="border rounded px-3 py-2"
        >
          {availableModels.map((model) => (
            <option key={model} value={model}>
              {model} - {getModelDescription(model, t)}
            </option>
          ))}
        </select>
      </div>
      
      {/* Example 4: Model grid */}
      <div className="border rounded-lg p-4">
        <h3 className="font-medium mb-3">4. Model Grid</h3>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          {AVAILABLE_MODELS.map((modelInfo) => {
            const modelName = t(modelInfo.key);
            const description = t(modelInfo.descriptionKey);
            const isSelected = modelName === selectedModel;
            
            return (
              <button
                key={modelInfo.id}
                onClick={() => setSelectedModel(modelName)}
                className={`p-3 border rounded-lg text-left transition-colors ${
                  isSelected 
                    ? 'border-blue-500 bg-blue-50' 
                    : 'border-gray-200 hover:border-gray-300'
                }`}
              >
                <div className="font-medium">{modelName}</div>
                <div className="text-sm text-gray-600">{description}</div>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}