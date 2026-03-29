// Performance monitoring and optimization utilities
class PerformanceMonitor {
  private metrics: Map<string, number[]> = new Map();
  private observers: Map<string, PerformanceObserver> = new Map();

  constructor() {
    this.initializeObservers();
  }

  private initializeObservers() {
    if (typeof window === 'undefined') return;

    // Measure navigation timing
    this.observeNavigationTiming();
    
    // Measure resource loading
    this.observeResourceTiming();
    
    // Measure layout shifts
    this.observeLayoutShifts();
    
    // Measure largest contentful paint
    this.observeLCP();
    
    // Measure first input delay
    this.observeFID();
  }

  private observeNavigationTiming() {
    if ('performance' in window && 'getEntriesByType' in performance) {
      const navigationEntry = performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming;
      
      if (navigationEntry) {
        this.recordMetric('navigation.loadComplete', navigationEntry.loadEventEnd - navigationEntry.loadEventStart);
        // Use domInteractive (supported) rather than domLoading (non-standard)
        this.recordMetric('navigation.domComplete', navigationEntry.domComplete - (navigationEntry as any).domInteractive);
        this.recordMetric('navigation.firstPaint', navigationEntry.responseEnd - navigationEntry.requestStart);
      }
    }
  }

  private observeResourceTiming() {
    if ('PerformanceObserver' in window) {
      const observer = new PerformanceObserver((list) => {
        list.getEntries().forEach((entry) => {
          if (entry.entryType === 'resource') {
            const resourceEntry = entry as PerformanceResourceTiming;
            this.recordMetric(`resource.${resourceEntry.initiatorType}`, resourceEntry.duration);
          }
        });
      });

      observer.observe({ entryTypes: ['resource'] });
      this.observers.set('resource', observer);
    }
  }

  private observeLayoutShifts() {
    if ('PerformanceObserver' in window) {
      const observer = new PerformanceObserver((list) => {
        list.getEntries().forEach((entry) => {
          if (entry.entryType === 'layout-shift' && !(entry as any).hadRecentInput) {
            this.recordMetric('cls', (entry as any).value);
          }
        });
      });

      observer.observe({ entryTypes: ['layout-shift'] });
      this.observers.set('layout-shift', observer);
    }
  }

  private observeLCP() {
    if ('PerformanceObserver' in window) {
      const observer = new PerformanceObserver((list) => {
        const entries = list.getEntries();
        const lastEntry = entries[entries.length - 1];
        if (lastEntry) {
          this.recordMetric('lcp', lastEntry.startTime);
        }
      });

      observer.observe({ entryTypes: ['largest-contentful-paint'] });
      this.observers.set('lcp', observer);
    }
  }

  private observeFID() {
    if ('PerformanceObserver' in window) {
      const observer = new PerformanceObserver((list) => {
        list.getEntries().forEach((entry) => {
          if (entry.entryType === 'first-input') {
            const fidEntry = entry as PerformanceEventTiming;
            this.recordMetric('fid', fidEntry.processingStart - fidEntry.startTime);
          }
        });
      });

      observer.observe({ entryTypes: ['first-input'] });
      this.observers.set('fid', observer);
    }
  }

  recordMetric(name: string, value: number) {
    if (!this.metrics.has(name)) {
      this.metrics.set(name, []);
    }
    
    const values = this.metrics.get(name)!;
    values.push(value);
    
    // Keep only last 100 measurements
    if (values.length > 100) {
      values.shift();
    }
  }

  getMetrics() {
    const result: Record<string, any> = {};
    
    this.metrics.forEach((values, name) => {
      if (values.length > 0) {
        const sorted = [...values].sort((a, b) => a - b);
        
        result[name] = {
          min: sorted[0],
          max: sorted[sorted.length - 1],
          avg: values.reduce((sum, val) => sum + val, 0) / values.length,
          p50: sorted[Math.floor(sorted.length * 0.5)],
          p90: sorted[Math.floor(sorted.length * 0.9)],
          p95: sorted[Math.floor(sorted.length * 0.95)],
          count: values.length,
        };
      }
    });
    
    return result;
  }

  logPerformanceReport() {
    const metrics = this.getMetrics();
    
    console.group('📊 Performance Report');
    
    // Core Web Vitals
    if (metrics.lcp) {
      console.log(`🎯 LCP: ${metrics.lcp.avg.toFixed(2)}ms (${this.getWebVitalScore('lcp', metrics.lcp.avg)})`);
    }
    
    if (metrics.fid) {
      console.log(`⚡ FID: ${metrics.fid.avg.toFixed(2)}ms (${this.getWebVitalScore('fid', metrics.fid.avg)})`);
    }
    
    if (metrics.cls) {
      console.log(`📐 CLS: ${metrics.cls.avg.toFixed(3)} (${this.getWebVitalScore('cls', metrics.cls.avg)})`);
    }
    
    // Navigation metrics
    if (metrics['navigation.loadComplete']) {
      console.log(`📄 Page Load: ${metrics['navigation.loadComplete'].avg.toFixed(2)}ms`);
    }
    
    if (metrics['navigation.domComplete']) {
      console.log(`🏗️ DOM Ready: ${metrics['navigation.domComplete'].avg.toFixed(2)}ms`);
    }
    
    // Resource metrics
    ['script', 'css', 'img', 'fetch'].forEach(type => {
      const key = `resource.${type}`;
      if (metrics[key]) {
        console.log(`📦 ${type.toUpperCase()}: ${metrics[key].avg.toFixed(2)}ms avg`);
      }
    });
    
    console.groupEnd();
  }

  private getWebVitalScore(metric: string, value: number): string {
    const thresholds = {
      lcp: { good: 2500, poor: 4000 },
      fid: { good: 100, poor: 300 },
      cls: { good: 0.1, poor: 0.25 },
    };
    
    const threshold = thresholds[metric as keyof typeof thresholds];
    if (!threshold) return 'unknown';
    
    if (value <= threshold.good) return '🟢 Good';
    if (value <= threshold.poor) return '🟡 Needs Improvement';
    return '🔴 Poor';
  }

  startTimer(name: string): () => void {
    const start = performance.now();
    
    return () => {
      const end = performance.now();
      this.recordMetric(name, end - start);
    };
  }

  measureAsync<T>(name: string, asyncFn: () => Promise<T>): Promise<T> {
    const endTimer = this.startTimer(name);
    
    return asyncFn().finally(() => {
      endTimer();
    });
  }

  disconnect() {
    this.observers.forEach(observer => {
      observer.disconnect();
    });
    this.observers.clear();
  }
}

// Global performance monitor instance
export const performanceMonitor = new PerformanceMonitor();

// React hook for component performance monitoring
export const usePerformanceTimer = (componentName: string) => {
  const timerRef = { current: null as (() => void) | null };
  
  const startTimer = () => {
    timerRef.current = performanceMonitor.startTimer(`component.${componentName}`);
  };
  
  const endTimer = () => {
    if (timerRef.current) {
      timerRef.current();
      timerRef.current = null;
    }
  };
  
  return { startTimer, endTimer };
};

// Bundle size monitoring
export const bundleAnalyzer = {
  logBundleInfo: () => {
    if (typeof window === 'undefined') return;
    
    console.group('📦 Bundle Analysis');
    
    // Estimate JavaScript bundle size
    const scripts = Array.from(document.querySelectorAll('script[src]'));
    let totalJSSize = 0;
    
    scripts.forEach(script => {
      const src = (script as HTMLScriptElement).src;
      if (src.includes('/_next/')) {
        // Estimate size based on common Next.js chunk patterns
        if (src.includes('framework')) {
          console.log(`⚛️ React Framework: ~40KB (estimated)`);
          totalJSSize += 40;
        } else if (src.includes('main')) {
          console.log(`🏠 Main Bundle: ~15KB (estimated)`);
          totalJSSize += 15;
        } else if (src.includes('webpack')) {
          console.log(`📦 Webpack Runtime: ~2KB (estimated)`);
          totalJSSize += 2;
        }
      }
    });
    
    console.log(`📊 Total JS (estimated): ~${totalJSSize}KB`);
    
    // CSS analysis
    const stylesheets = Array.from(document.querySelectorAll('link[rel="stylesheet"]'));
    console.log(`🎨 CSS Files: ${stylesheets.length}`);
    
    console.groupEnd();
  },
};

// Memory usage monitoring
export const memoryMonitor = {
  logMemoryUsage: () => {
    if (typeof window === 'undefined' || !('memory' in performance)) return;
    
    const memory = (performance as any).memory;
    
    console.group('🧠 Memory Usage');
    console.log(`💾 Used: ${(memory.usedJSHeapSize / 1024 / 1024).toFixed(2)} MB`);
    console.log(`📊 Total: ${(memory.totalJSHeapSize / 1024 / 1024).toFixed(2)} MB`);
    console.log(`🚫 Limit: ${(memory.jsHeapSizeLimit / 1024 / 1024).toFixed(2)} MB`);
    console.groupEnd();
  },
  
  startMonitoring: (intervalMs = 10000) => {
    return setInterval(() => {
      memoryMonitor.logMemoryUsage();
    }, intervalMs);
  },
};

// Performance optimization recommendations
export const performanceOptimizer = {
  analyzeAndSuggest: () => {
    const metrics = performanceMonitor.getMetrics();
    const suggestions: string[] = [];
    
    // LCP optimization
    if (metrics.lcp && metrics.lcp.avg > 2500) {
      suggestions.push('🎯 Optimize Largest Contentful Paint: Consider lazy loading, image optimization, or reducing server response time');
    }
    
    // FID optimization
    if (metrics.fid && metrics.fid.avg > 100) {
      suggestions.push('⚡ Optimize First Input Delay: Consider code splitting, reducing JavaScript execution time, or using web workers');
    }
    
    // CLS optimization
    if (metrics.cls && metrics.cls.avg > 0.1) {
      suggestions.push('📐 Optimize Cumulative Layout Shift: Ensure proper image/video dimensions, avoid dynamic content insertion');
    }
    
    // Resource optimization
    if (metrics['resource.script'] && metrics['resource.script'].avg > 1000) {
      suggestions.push('📦 Optimize JavaScript loading: Consider code splitting, tree shaking, or script optimization');
    }
    
    if (metrics['resource.css'] && metrics['resource.css'].avg > 500) {
      suggestions.push('🎨 Optimize CSS loading: Consider critical CSS inlining or CSS purging');
    }
    
    if (suggestions.length > 0) {
      console.group('💡 Performance Optimization Suggestions');
      suggestions.forEach(suggestion => console.log(suggestion));
      console.groupEnd();
    } else {
      console.log('✅ No obvious performance issues detected!');
    }
    
    return suggestions;
  },
};

// Auto-report every 30 seconds in development
if (typeof window !== 'undefined' && process.env.NODE_ENV === 'development') {
  let reportCount = 0;
  const maxReports = 5; // Limit to prevent spam
  
  const reportInterval = setInterval(() => {
    if (reportCount >= maxReports) {
      clearInterval(reportInterval);
      console.log('📊 Performance monitoring reports completed');
      return;
    }
    
    performanceMonitor.logPerformanceReport();
    performanceOptimizer.analyzeAndSuggest();
    memoryMonitor.logMemoryUsage();
    bundleAnalyzer.logBundleInfo();
    
    reportCount++;
  }, 30000);
}

export default performanceMonitor;
