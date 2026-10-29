# frozen_string_literal: true
require 'date'
require 'digest'
require 'json'
require 'open-uri'
require 'yaml'

commit = 'fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f'
files = { 'accounting' => 'xero_accounting.yaml', 'assets' => 'xero_assets.yaml',
          'bankfeeds' => 'xero_bankfeeds.yaml', 'projects' => 'xero-projects.yaml',
          'identity' => 'xero-identity.yaml', 'finance' => 'xero-finance.yaml',
          # Upstream Xero MCP's payroll tools use the New Zealand API family.
          'payroll_nz' => 'xero-payroll-nz.yaml' }
sources = {}
operations = files.flat_map do |family, file|
  raw = URI.open("https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/#{commit}/#{file}", &:read)
  spec = YAML.safe_load(raw, permitted_classes: [Date, Time])
  sources[family] = { 'file' => file, 'sha256' => Digest::SHA256.hexdigest(raw) }
  resolve = lambda do |node|
    node.key?('$ref') ? node['$ref'].split('/').drop(1).reduce(spec) { |obj, key| obj.fetch(key) } : node
  end
  spec.fetch('paths').map do |path, methods|
    get = methods['get']
    next unless get
    parameters = (methods.fetch('parameters', []) + get.fetch('parameters', [])).map do |param|
      param = resolve.call(param)
      schema = resolve.call(param.fetch('schema', {}))
      { 'name' => param.fetch('name'), 'location' => param.fetch('in'),
        'required' => param.fetch('required', false), 'schema' => schema,
        'description' => param.fetch('description', '') }
    end.reject { |p| p['name'].downcase == 'xero-tenant-id' }
    types = get.fetch('responses').map { |status, response| resolve.call(response).fetch('content', {}).keys if status.to_i.between?(200, 299) }.compact.flatten.uniq
    { 'family' => family, 'operation_id' => get.fetch('operationId'),
      'path' => get.fetch('x-path', path), 'base_url' => spec.fetch('servers').first.fetch('url'),
      'summary' => get.fetch('summary', get.fetch('description', '')).gsub(/\s+/, ' ').strip,
      'parameters' => parameters, 'response_content_types' => types,
      'documented_scopes' => get.fetch('security', spec.fetch('security', [])).flat_map(&:values).flatten.uniq }
  end.compact
end.sort_by { |op| [op['family'], op['operation_id']] }
abort 'expected 126 Accounting GET operations' unless operations.count { |op| op['family'] == 'accounting' } == 126
abort 'expected 36 Payroll NZ GET operations' unless operations.count { |op| op['family'] == 'payroll_nz' } == 36
abort 'expected 186 total GET operations' unless operations.length == 186
abort 'duplicate operation names' unless operations.map { |op| [op['family'], op['operation_id']] }.uniq.length == operations.length
warn "Generated #{operations.count { |op| op['family'] != 'accounting' }} extended GET operations"
File.write(ARGV.fetch(0), JSON.pretty_generate({ 'spec_commit' => commit, 'sources' => sources, 'operations' => operations }) + "\n")
