# frozen_string_literal: true

require_relative 'batch_machines/version'

module BatchMachines
  # The core's `Admission::Flush(FlushReason)`: the item was counted and a
  # flush is due. `reason` is :size or :deadline. The other honoured push
  # answer, `Admission::Buffered`, is the symbol :buffered.
  Flush = Data.define(:reason)
end

# Native is required: there is no pure-Ruby fallback. It builds Flush values,
# so the Data class above must exist first.
require 'batch_machines_native/batch_machines_native'
