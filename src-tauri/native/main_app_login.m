#import <Foundation/Foundation.h>
#import <ServiceManagement/ServiceManagement.h>
#include <stdlib.h>
#include <string.h>

static NSString *state(SMAppServiceStatus status) {
    switch (status) {
        case SMAppServiceStatusNotRegistered: return @"notRegistered";
        case SMAppServiceStatusEnabled: return @"enabled";
        case SMAppServiceStatusRequiresApproval: return @"requiresApproval";
        case SMAppServiceStatusNotFound: return @"notFound";
        default: return @"unknown";
    }
}
// Only the calling main App. No helper ID, plist, executable, URL or argv input.
char *moomole_main_app_login(int operation, const char *expectedId) {
    @autoreleasepool {
        NSDictionary *result;
        @try {
            NSBundle *bundle = NSBundle.mainBundle;
            NSString *expected = expectedId ? [NSString stringWithUTF8String:expectedId] : nil;
            NSString *path = bundle.bundleURL.path;
            if (!expected.length || ![bundle.bundleIdentifier isEqualToString:expected] ||
                ![path.pathExtension isEqualToString:@"app"] || !bundle.executableURL.isFileURL ||
                ![bundle.executableURL.path hasPrefix:[path stringByAppendingString:@"/Contents/MacOS/"]] ||
                NSThread.isMainThread) {
                result = @{ @"error": @"requiresPackagedApp" };
            } else if (operation < 0 || operation > 2) {
                result = @{ @"error": @"invalidRequest" };
            } else if (@available(macOS 13.0, *)) {
                SMAppService *service = SMAppService.mainAppService;
                BOOL accepted = YES;
                NSError *error = nil;
                if (operation == 1 && service.status != SMAppServiceStatusEnabled && service.status != SMAppServiceStatusRequiresApproval)
                    accepted = [service registerAndReturnError:&error];
                else if (operation == 2 && (service.status == SMAppServiceStatusEnabled || service.status == SMAppServiceStatusRequiresApproval))
                    accepted = [service unregisterAndReturnError:&error];
                else if (operation != 0 && service.status == SMAppServiceStatusNotFound)
                    accepted = NO;
                NSMutableDictionary *body = [@{
                    @"system": @{ @"bundleId": expected, @"bundlePath": path, @"status": state(service.status) },
                    @"accepted": (accepted ? @YES : @NO)
                } mutableCopy];
                if (!accepted || error) body[@"error"] = @"requestRejected";
                if (error) body[@"nativeCode"] = @(error.code);
                result = body;
            } else result = @{ @"error": @"unsupportedSystem" };
        } @catch (NSException *exception) { result = @{ @"error": @"nativeUnavailable" }; }
        NSData *data = [NSJSONSerialization dataWithJSONObject:result options:0 error:nil];
        if (!data || data.length > 16384) return NULL;
        char *output = malloc(data.length + 1);
        if (output) {memcpy(output, data.bytes, data.length); output[data.length] = 0;}
        return output;
    }
}
