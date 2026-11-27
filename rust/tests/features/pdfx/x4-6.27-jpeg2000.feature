Feature: PDF/X-4 6.27, JPEG 2000 images
  JPEG 2000 data is the JPX file format using only the JPX baseline features, with 1, 3 or 4 channels, at most one
  colour specification, no CIEJab, and bit depths of 1, 8 or 16, the same in every channel; profile-less CMYK with no
  /ColorSpace counts as DeviceCMYK. Its header is valid (6.8); the codestream is not decoded.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | check |
      | x4-jpx-five-channels | x4.jpx.channels |
      | x4-jpx-two-colours | x4.jpx.one-colour-space |
      | x4-jpx-ciejab | x4.jpx.no-ciejab |
      | x4-jpx-depth-12 | x4.jpx.bit-depth |
      | x4-jpx-mixed-depths | x4.jpx.uniform-depth |
      | x4-jpx-plain-jp2 | x4.jpx.baseline |
      | x4-jpx-bare-codestream | x4.jpx.baseline |
      | x4-jpx-not-jpeg2000 | x4.image.jpx-valid |

  Scenario: CMYK JPEG 2000 with no profile and no /ColorSpace is DeviceCMYK
    Given the fixture "x4-jpx-cmyk-as-device"
    When it is verified as PDF/X-4
    Then check "x4.jpx.cmyk-as-device" fails
