package mailtask

import "testing"

func TestHTMLToText(t *testing.T) {
	tests := []struct {
		name string
		html string
		want string
	}{
		{"empty", "", ""},
		{"plain text passes through", "just text", "just text"},
		{"paragraphs are separated by one blank line", "<p>One</p><p>Two</p>", "One\n\nTwo"},
		{"divs end the line without a blank line", "<div>One</div>\n<div>Two</div>", "One\nTwo"},
		{"br and self-closing br", "A<br>B<br/>C<BR />D", "A\nB\nC\nD"},
		{"source newlines are layout, not breaks", "<p>one\n   two\tthree</p>", "one two three"},
		{"table cells are separated, rows break", "<table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>", "a b\nc d"},
		{"lists", "<ul>\n<li>x</li>\n<li>y</li>\n</ul><ol><li>z</li></ol>", "- x\n- y\n\n- z"},
		{"gt inside a quoted attribute does not end the tag", `<a href="x" title="a > b">link</a> after`, "link after"},
		{"apostrophe in unquoted text inside a tag does not swallow the document", `<span data-x=it's>kept</span> and more`, "kept and more"},
		{"stray less-than is text", "<p>a < b and 3<4</p>", "a < b and 3<4"},
		{"less-than at the very end", "end <", "end <"},
		{"img keeps nothing", `before<img src="x.png" alt="SECRET-ALT">after`, "beforeafter"},
		{"closing script tag with whitespace and attributes", `a<script>x()</script  foo="1">b`, "ab"},
		{"script content that looks like tags", `a<script>if (1<2) { document.write("<p>no</p>") }</script>b`, "ab"},
		{"style inside body", "a<style>p{content:'<p>'}</style>b", "ab"},
		{"svg and noscript are dropped", `a<svg><title>icon</title><text>t</text></svg><noscript>enable js</noscript>b`, "ab"},
		{"title element in head is dropped", "<html><head><title>T</title></head><body>Body</body></html>", "Body"},
		{"conditional comment is dropped", "a<!--[if mso]><p>outlook only</p><![endif]-->b", "ab"},
		{"downlevel-revealed conditional markers are dropped", "a<![if !mso]>b<![endif]>c", "abc"},
		{"unterminated comment drops the rest", "a<!-- never closed <p>b</p>", "a"},
		{"unterminated tag drops the rest", `a<p class="x`, "a"},
		{"named, decimal and hex entities", "&lt;tag&gt; &amp; &#65;&#x42; &euro; &nbsp;|", "<tag> & AB € |"},
		{"an escaped tag stays text and is not re-parsed", "&lt;script&gt;alert(1)&lt;/script&gt;", "<script>alert(1)</script>"},
		{"unicode text is preserved", "<p>Blåbærsyltetøy – æøå 日本語</p>", "Blåbærsyltetøy – æøå 日本語"},
		{"invisible preheader padding is removed", "<p>Hi&#x200b;&zwnj;&#x34f;&shy;&#xfeff; there</p>", "Hi there"},
		{"blank lines collapse", "<div>a</div><div></div><div></div><div>b</div>", "a\nb"},
		{"nested blocks do not stack blank lines", "<div><div><p>a</p></div></div>", "a"},
		{"uppercase tags", "<P>UP</P><DIV>DOWN</DIV>", "UP\n\nDOWN"},
		{"explicit breaks are kept, runs of them collapse to one blank line", "a<br><br><br><br>b", "a\n\nb"},
		{"pretty-printed whitespace between tags adds no blank lines", "<div>\n  <div>\n    a\n  </div>\n  <div>\n    b\n  </div>\n</div>", "a\nb"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := htmlToText(test.html); got != test.want {
				t.Fatalf("htmlToText(%q) = %q, want %q", test.html, got, test.want)
			}
		})
	}
}

func TestHTMLToTextNeverPanicsOnHostileInput(t *testing.T) {
	for _, html := range []string{
		"<", "<<<<", "<a", "<a ", `<a href="`, "<!--", "<!---->", "<script", "<script>", "</script>", "<style", "</",
		"<!", "<?", "<?php echo 1 ?>", "\x00<p>\x00</p>\x00", "<p>\xff\xfe</p>", "<a b=>", `<a b='>`,
	} {
		_ = htmlToText(html) // must terminate without panicking
	}
}
