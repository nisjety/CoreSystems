package crawl

import "regexp"

func CompileRegexp(pattern string) (*regexp.Regexp, error) {
	return regexp.Compile(pattern)
}
