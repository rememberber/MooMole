// Local exported resources only. This API does not access the Photos library.
#import <Photos/Photos.h>
#import <Foundation/Foundation.h>
#import <AppKit/AppKit.h>
#import <CoreFoundation/CoreFoundation.h>
#include <stdint.h>

int moomole_live_photo_validate(const char *photo, const char *movie, uint32_t *width, uint32_t *height) {
    @autoreleasepool {
        if (![NSThread isMainThread] || !photo || !movie || !width || !height) return -1;
        // PhotoKit requires the AppKit application environment even in a
        // resource-only worker. Never activate or create a window/Dock UI.
        NSApplication *application = NSApp ?: [NSApplication sharedApplication];
        [application setActivationPolicy:NSApplicationActivationPolicyProhibited];
        NSURL *imageURL = [NSURL fileURLWithPath:[NSString stringWithUTF8String:photo]];
        NSURL *videoURL = [NSURL fileURLWithPath:[NSString stringWithUTF8String:movie]];
        __block BOOL done = NO, accepted = NO;
        __block CGSize size = CGSizeZero;
        PHLivePhotoRequestID request = [PHLivePhoto requestLivePhotoWithResourceFileURLs:@[imageURL, videoURL] placeholderImage:nil targetSize:CGSizeMake(320, 320) contentMode:PHImageContentModeAspectFit resultHandler:^(PHLivePhoto *live, NSDictionary *info) {
#ifdef MOOMOLE_LIVE_PROBE
            NSLog(@"Owned probe Live Photo result: %@", info);
#endif
            if ([info[PHLivePhotoInfoIsDegradedKey] boolValue]) return;
            accepted = live != nil && !info[PHLivePhotoInfoErrorKey] && ![info[PHLivePhotoInfoCancelledKey] boolValue];
            if (accepted) size = live.size;
            done = YES;
        }];
        NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:8];
        while (!done && deadline.timeIntervalSinceNow > 0) {
            [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.02]];
        }
        if (!done) { [PHLivePhoto cancelLivePhotoRequestWithRequestID:request]; return -1; }
        if (!accepted || !isfinite(size.width) || !isfinite(size.height) || size.width < 1 || size.height < 1 || size.width > 60000 || size.height > 60000) return 0;
        *width = (uint32_t)size.width; *height = (uint32_t)size.height;
        return 1;
    }
}
#ifdef MOOMOLE_LIVE_PROBE
#include <stdio.h>
int main(int argc, char **argv) {
    uint32_t width = 0, height = 0;
    if (argc != 3) return 2;
    int result = moomole_live_photo_validate(argv[1], argv[2], &width, &height);
    printf("result=%d width=%u height=%u\n", result, width, height);
    return result == 1 ? 0 : 1;
}
#endif
