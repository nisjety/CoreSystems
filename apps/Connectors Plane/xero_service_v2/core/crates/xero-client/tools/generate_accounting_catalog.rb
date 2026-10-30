# frozen_string_literal: true

require 'date'
require 'digest'
require 'json'
require 'yaml'

SPEC_COMMIT = 'fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f'
raw = STDIN.read
abort 'empty Xero OpenAPI input' if raw.empty?
spec = YAML.safe_load(raw, permitted_classes: [Date, Time])
refs = spec.fetch('components').fetch('parameters')

operations = spec.fetch('paths').map do |path, methods|
  get = methods['get']
  next unless get

  params = (methods.fetch('parameters', []) + get.fetch('parameters', [])).map do |param|
    param = refs.fetch(param.fetch('$ref').split('/').last) if param.key?('$ref')
    schema = param.fetch('schema', {})
    {
      'name' => param.fetch('name'),
      'location' => param.fetch('in'),
      'required' => param.fetch('required', false),
      'type' => schema.fetch('type', 'string'),
      'format' => schema['format']
    }
  end
  media = get.fetch('responses').map do |status, response|
    response.fetch('content', {}).keys if status.to_i.between?(200, 299)
  end.flatten.uniq.sort
  {
    'operation_id' => get.fetch('operationId'),
    'path' => get.fetch('x-path', path),
    'parameters' => params,
    'response_content_types' => media
  }
end.compact.sort_by { |op| op.fetch('operation_id') }

abort "expected 126 Accounting GETs, got #{operations.length}" unless operations.length == 126
abort 'duplicate operation ID' unless operations.map { |op| op.fetch('operation_id') }.uniq.length == 126

catalog = {
  'spec_commit' => SPEC_COMMIT,
  'spec_sha256' => Digest::SHA256.hexdigest(raw),
  'operations' => operations
}
File.write(ARGV.fetch(0), JSON.pretty_generate(catalog) + "\n")
if ARGV[1]
  loader_catalog = {
    'spec_commit' => SPEC_COMMIT,
    'operations' => operations.to_h do |operation|
      [operation.fetch('operation_id'),
       (operation.fetch('response_content_types') &
         ['application/pdf', 'application/octet-stream']).any?]
    end
  }
  File.write(ARGV[1], JSON.pretty_generate(loader_catalog) + "\n")
end
