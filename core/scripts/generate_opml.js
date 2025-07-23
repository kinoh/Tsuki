#!/usr/bin/env node

/**
 * OPML Generator Script
 * Generates OPML file from RSS feed URLs organized by categories
 */

import fs from 'fs';
import path from 'path';

// RSS feed URLs organized by categories
const rssFeeds = {
  "Tech News": [
    "https://feeds.feedburner.com/oreilly/radar",
    "https://techcrunch.com/feed/",
    "https://www.theverge.com/rss/index.xml",
    "https://arstechnica.com/feed/",
    "https://www.wired.com/feed/rss"
  ],
  // "Development": [
  //   "https://github.blog/feed/",
  //   "https://stackoverflow.blog/feed/",
  //   "https://dev.to/feed",
  //   "https://blog.github.com/feed.xml",
  //   "https://css-tricks.com/feed/"
  // ],
  "AI/ML": [
    "https://openai.com/blog/rss.xml",
    "https://blog.google/technology/ai/rss/",
    "https://www.anthropic.com/news/rss.xml",
    "https://machinelearningmastery.com/feed/",
    "https://ai.googleblog.com/feeds/posts/default"
  ],
  "Japanese Tech": [
    "https://zenn.dev/feed",
    "https://qiita.com/popular-items/feed",
    "https://gihyo.jp/feed/rss2",
    "https://www.publickey1.jp/atom.xml"
  ],
  "Japanese News": [
    "https://www.nhk.or.jp/rss/news/cat0.xml",
    "http://feeds.afpbb.com/rss/afpbb/afpbbnews",
    "https://feeds.cnn.co.jp/rss/cnn/cnn.rdf",
    "https://www.newsweekjapan.jp/story/rss.xml",
  ],
};

/**
 * Generate OPML XML content
 * @param {Object} feeds - RSS feeds organized by categories
 * @returns {string} OPML XML content
 */
function generateOPML(feeds) {
  const now = new Date().toUTCString();
  
  let opml = `<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head>
    <title>Tsuki RSS Feeds</title>
    <dateCreated>${now}</dateCreated>
    <dateModified>${now}</dateModified>
    <ownerName>Tsuki Agent</ownerName>
    <ownerEmail>tsuki@example.com</ownerEmail>
  </head>
  <body>
`;

  // Generate outline elements for each category
  for (const [category, urls] of Object.entries(feeds)) {
    opml += `    <outline text="${escapeXML(category)}" title="${escapeXML(category)}">\n`;
    
    for (const url of urls) {
      // Extract site name from URL for display
      const siteName = extractSiteName(url);
      opml += `      <outline type="rss" text="${escapeXML(siteName)}" title="${escapeXML(siteName)}" xmlUrl="${escapeXML(url)}" htmlUrl="${escapeXML(getBaseUrl(url))}" />\n`;
    }
    
    opml += `    </outline>\n`;
  }

  opml += `  </body>
</opml>`;

  return opml;
}

/**
 * Escape XML special characters
 * @param {string} text 
 * @returns {string}
 */
function escapeXML(text) {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/**
 * Extract site name from RSS URL
 * @param {string} url 
 * @returns {string}
 */
function extractSiteName(url) {
  try {
    const urlObj = new URL(url);
    let hostname = urlObj.hostname;
    
    // Remove www. prefix
    hostname = hostname.replace(/^www\./, '');
    
    // Convert to title case
    return hostname.split('.')[0]
      .split(/[-_]/)
      .map(word => word.charAt(0).toUpperCase() + word.slice(1))
      .join(' ');
  } catch (error) {
    return url;
  }
}

/**
 * Get base URL from RSS URL
 * @param {string} url 
 * @returns {string}
 */
function getBaseUrl(url) {
  try {
    const urlObj = new URL(url);
    return `${urlObj.protocol}//${urlObj.hostname}`;
  } catch (error) {
    return url;
  }
}

/**
 * Main function
 */
function main() {
  const outputFile = process.argv[2] || 'tsuki-feeds.opml';
  const outputPath = path.resolve(outputFile);
  
  console.log('🚀 Generating OPML file...');
  console.log(`📁 Output: ${outputPath}`);
  
  try {
    const opmlContent = generateOPML(rssFeeds);
    fs.writeFileSync(outputPath, opmlContent, 'utf8');
    
    console.log('✅ OPML file generated successfully!');
    console.log(`📊 Categories: ${Object.keys(rssFeeds).length}`);
    console.log(`📡 Total feeds: ${Object.values(rssFeeds).reduce((sum, feeds) => sum + feeds.length, 0)}`);
    
    // Display summary
    console.log('\n📋 Feed Summary:');
    for (const [category, urls] of Object.entries(rssFeeds)) {
      console.log(`  ${category}: ${urls.length} feeds`);
    }
    
  } catch (error) {
    console.error('❌ Error generating OPML file:', error.message);
    process.exit(1);
  }
}

main();
