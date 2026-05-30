package driver

import (
	"fmt"
	"math/rand"
	"strings"
	"sync"
	"time"
)

// BrowserFingerprint holds all elements of a realistic browser identity.
// Every field is internally consistent: a macOS UA matches a macOS platform
// string and macOS-compatible WebGL renderer, etc.
type BrowserFingerprint struct {
	UserAgent           string
	Platform            string // e.g. "Win32", "MacIntel", "Linux x86_64"
	Vendor              string // "Google Inc."
	SecChUa             string
	SecChUaPlatform     string
	SecChUaMobile       string
	ViewportWidth       int
	ViewportHeight      int
	ScreenWidth         int
	ScreenHeight        int
	DevicePixelRatio    float64
	ColorDepth          int
	HardwareConcurrency int
	DeviceMemory        int // GB
	Languages           []string
	WebGLVendor         string
	WebGLRenderer       string
	OsCPU               string // mirrors navigator.oscpu
}

// fingerprint randomisation pool — all values are from common real-world
// browser populations to avoid statistical outlier detection.

type platformSpec struct {
	os           string // "windows", "macos", "linux"
	platform     string
	oscpu        string
	secChPlatf   string
	webglVendors []string
	webglRenders []string
}

var platformSpecs = []platformSpec{
	{
		os: "windows", platform: "Win32",
		oscpu: "Windows NT 10.0; Win64; x64", secChPlatf: `"Windows"`,
		webglVendors: []string{"Google Inc. (NVIDIA)", "Google Inc. (AMD)", "Google Inc. (Intel)"},
		webglRenders: []string{
			"ANGLE (NVIDIA, NVIDIA GeForce RTX 3060 Direct3D11 vs_5_0 ps_5_0)",
			"ANGLE (NVIDIA, NVIDIA GeForce GTX 1660 SUPER Direct3D11 vs_5_0 ps_5_0)",
			"ANGLE (AMD, AMD Radeon RX 580 Direct3D11 vs_5_0 ps_5_0)",
			"ANGLE (Intel, Intel(R) UHD Graphics 630 Direct3D11 vs_5_0 ps_5_0)",
			"ANGLE (NVIDIA, NVIDIA GeForce RTX 4070 Direct3D11 vs_5_0 ps_5_0)",
		},
	},
	{
		os: "macos", platform: "MacIntel",
		oscpu: "Intel Mac OS X 10_15_7", secChPlatf: `"macOS"`,
		webglVendors: []string{"Google Inc. (Apple)", "Google Inc. (Intel Inc.)"},
		webglRenders: []string{
			"ANGLE (Apple, Apple M1, OpenGL 4.1)",
			"ANGLE (Apple, Apple M2, OpenGL 4.1)",
			"ANGLE (Apple, Apple M3, OpenGL 4.1)",
			"ANGLE (Intel Inc., Intel(R) Iris(TM) Plus Graphics, OpenGL 4.1)",
		},
	},
	{
		os: "linux", platform: "Linux x86_64",
		oscpu: "Linux x86_64", secChPlatf: `"Linux"`,
		webglVendors: []string{"Google Inc. (Mesa)", "Google Inc. (NVIDIA Corporation)"},
		webglRenders: []string{
			"ANGLE (Mesa, llvmpipe (LLVM 15.0.7, 256 bits), OpenGL 4.5)",
			"ANGLE (NVIDIA Corporation, NVIDIA GeForce GTX 1080/PCIe/SSE2, OpenGL 4.6)",
		},
	},
}

// Chrome versions we rotate through (recent, not too bleeding edge).
var chromeVersions = []struct {
	major     int
	fullVer   string
	notABrand string
}{
	{131, "131.0.6778.86", `"Not_A Brand";v="8"`},
	{130, "130.0.6723.117", `"Not?A_Brand";v="99"`},
	{129, "129.0.6668.58", `"Not=A?Brand";v="8"`},
	{128, "128.0.6613.120", `"Not;A=Brand";v="8"`},
	{127, "127.0.6533.73", `"Not/A)Brand";v="8"`},
	{126, "126.0.6478.127", `"Not/A)Brand";v="8"`},
	{133, "133.0.6943.53", `"Not(A:Brand";v="99"`},
}

// Common screen resolutions with typical DPRs.
var viewports = []struct {
	w, h int
	dprs []float64
}{
	{1920, 1080, []float64{1, 1.25}},
	{1366, 768, []float64{1, 1.25}},
	{1536, 864, []float64{1.25}},
	{1440, 900, []float64{1, 2}},
	{1280, 720, []float64{1, 1.5}},
	{2560, 1440, []float64{1, 1.5}},
	{1680, 1050, []float64{1}},
}

var languageSets = [][]string{
	{"en-US", "en"},
	{"en-GB", "en"},
	{"en-US", "en", "de"},
	{"en-US", "en", "fr"},
	{"en-US", "en", "es"},
}

var hwConcurrencies = []int{4, 8, 8, 8, 12, 16} // weighted toward 8
var deviceMemories = []int{4, 8, 8, 16}         // weighted toward 8

var fpRand struct {
	mu   sync.Mutex
	rand *rand.Rand
}

func init() {
	fpRand.rand = rand.New(rand.NewSource(time.Now().UnixNano()))
}

func fpRandIntn(n int) int {
	fpRand.mu.Lock()
	defer fpRand.mu.Unlock()
	return fpRand.rand.Intn(n)
}

// GenerateFingerprint creates a realistic, internally consistent browser
// fingerprint suitable for injection into headless Chrome and HTTP header
// construction.
func GenerateFingerprint() BrowserFingerprint {
	// Pick platform.
	spec := platformSpecs[fpRandIntn(len(platformSpecs))]

	// Pick Chrome version.
	cv := chromeVersions[fpRandIntn(len(chromeVersions))]

	// Pick viewport.
	vp := viewports[fpRandIntn(len(viewports))]
	dpr := vp.dprs[fpRandIntn(len(vp.dprs))]

	// Pick languages, hardware, memory.
	langs := languageSets[fpRandIntn(len(languageSets))]
	hwc := hwConcurrencies[fpRandIntn(len(hwConcurrencies))]
	devMem := deviceMemories[fpRandIntn(len(deviceMemories))]

	// Pick WebGL.
	glVendor := spec.webglVendors[fpRandIntn(len(spec.webglVendors))]
	glRenderer := spec.webglRenders[fpRandIntn(len(spec.webglRenders))]

	// Build User-Agent.
	var ua string
	switch spec.os {
	case "windows":
		ua = fmt.Sprintf("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/%s Safari/537.36", cv.fullVer)
	case "macos":
		ua = fmt.Sprintf("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/%s Safari/537.36", cv.fullVer)
	case "linux":
		ua = fmt.Sprintf("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/%s Safari/537.36", cv.fullVer)
	}

	// Build Sec-Ch-Ua.
	secChUa := fmt.Sprintf(`"Chromium";v="%d", "Google Chrome";v="%d", %s`, cv.major, cv.major, cv.notABrand)

	return BrowserFingerprint{
		UserAgent:           ua,
		Platform:            spec.platform,
		Vendor:              "Google Inc.",
		SecChUa:             secChUa,
		SecChUaPlatform:     spec.secChPlatf,
		SecChUaMobile:       "?0",
		ViewportWidth:       vp.w,
		ViewportHeight:      vp.h,
		ScreenWidth:         vp.w,
		ScreenHeight:        vp.h,
		DevicePixelRatio:    dpr,
		ColorDepth:          24,
		HardwareConcurrency: hwc,
		DeviceMemory:        devMem,
		Languages:           langs,
		WebGLVendor:         glVendor,
		WebGLRenderer:       glRenderer,
		OsCPU:               spec.oscpu,
	}
}

// AcceptLanguage returns the Accept-Language header value for this fingerprint.
func (fp BrowserFingerprint) AcceptLanguage() string {
	if len(fp.Languages) == 0 {
		return "en-US,en;q=0.9"
	}
	var parts []string
	for i, lang := range fp.Languages {
		if i == 0 {
			parts = append(parts, lang)
		} else {
			q := 1.0 - float64(i)*0.1
			if q < 0.1 {
				q = 0.1
			}
			parts = append(parts, fmt.Sprintf("%s;q=%.1f", lang, q))
		}
	}
	return strings.Join(parts, ",")
}

// HTTPHeaders returns the standard browser headers derived from this
// fingerprint, suitable for injection into HTTP requests.
func (fp BrowserFingerprint) HTTPHeaders() map[string]string {
	return map[string]string{
		"User-Agent":                fp.UserAgent,
		"Accept":                    "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8",
		"Accept-Language":           fp.AcceptLanguage(),
		"Accept-Encoding":           "gzip, deflate, br",
		"Sec-Ch-Ua":                 fp.SecChUa,
		"Sec-Ch-Ua-Mobile":          fp.SecChUaMobile,
		"Sec-Ch-Ua-Platform":        fp.SecChUaPlatform,
		"Sec-Fetch-Dest":            "document",
		"Sec-Fetch-Mode":            "navigate",
		"Sec-Fetch-Site":            "none",
		"Sec-Fetch-User":            "?1",
		"Upgrade-Insecure-Requests": "1",
	}
}
